#![allow(clippy::expect_used)]

use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use codex_exec_server_protocol::JSONRPCMessage;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use test_case::test_case;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::DuplexStream;
use tokio::io::duplex;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::ExecServerClient;
use crate::ExecServerClientConnectOptions;
use crate::ExecServerRuntimePaths;
use crate::ExecServerTelemetry;
use crate::FsWriteFileIfUnchangedParams;
use crate::connection::JsonRpcConnection;
use crate::file_system_mutation::test_support::Pause;
use crate::file_system_mutation::test_support::Phase;
use crate::file_system_mutation::test_support::available_permits;
use crate::protocol::FS_WRITE_FILE_IF_UNCHANGED_METHOD;
use crate::server::ConcurrentRequestLimit;
use crate::server::RequestDispatchMode;
use crate::server::processor::ConnectionProcessor;
use crate::telemetry::ConnectionTransport;

struct Connection {
    client: ExecServerClient,
    disconnect: CancellationToken,
    pumps: Vec<JoinHandle<()>>,
    server: JoinHandle<()>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Connection {
    async fn connect(processor: ConnectionProcessor) -> Self {
        let (client_writer, request_reader) = duplex(/*max_buf_size*/ 65536);
        let (request_writer, server_reader) = duplex(/*max_buf_size*/ 65536);
        let (server_writer, reply_reader) = duplex(/*max_buf_size*/ 65536);
        let (reply_writer, client_reader) = duplex(/*max_buf_size*/ 65536);
        let disconnect = CancellationToken::new();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let pumps = vec![
            tokio::spawn(pump(
                request_reader,
                request_writer,
                disconnect.clone(),
                Some(Arc::clone(&requests)),
            )),
            tokio::spawn(pump(
                reply_reader,
                reply_writer,
                disconnect.clone(),
                /*requests*/ None,
            )),
        ];
        let server = tokio::spawn(async move {
            processor
                .run_connection(
                    JsonRpcConnection::from_stdio(
                        server_reader,
                        server_writer,
                        "conditional server".to_string(),
                    ),
                    ConnectionTransport::Stdio,
                )
                .await;
        });
        let client = ExecServerClient::connect(
            JsonRpcConnection::client_from_stdio(
                client_reader,
                client_writer,
                "conditional client".to_string(),
            ),
            ExecServerClientConnectOptions {
                client_name: "conditional fixture".to_string(),
                initialize_timeout: Duration::from_secs(/*secs*/ 10),
                resume_session_id: None,
            },
        )
        .await
        .expect("actual initialize handshake");
        Self {
            client,
            disconnect,
            pumps,
            server,
            requests,
        }
    }

    async fn close(self) -> Vec<String> {
        self.disconnect.cancel();
        drop(self.client);
        for pump in self.pumps {
            timeout(Duration::from_secs(/*secs*/ 10), pump)
                .await
                .expect("transport closed")
                .expect("transport task");
        }
        timeout(Duration::from_secs(/*secs*/ 10), self.server)
            .await
            .expect("server handles disconnect")
            .expect("server task");
        let requests = self.requests.lock().expect("captured requests").clone();
        requests
    }
}

async fn pump(
    reader: DuplexStream,
    mut writer: DuplexStream,
    disconnect: CancellationToken,
    requests: Option<Arc<Mutex<Vec<String>>>>,
) {
    let mut lines = BufReader::new(reader).lines();
    loop {
        let line = tokio::select! {
            _ = disconnect.cancelled() => break,
            line = lines.next_line() => match line {
                Ok(Some(line)) => line,
                Ok(None) | Err(_) => break,
            },
        };
        if let Some(requests) = &requests
            && let JSONRPCMessage::Request(request) =
                serde_json::from_str(&line).expect("actual JSON-RPC")
        {
            requests
                .lock()
                .expect("captured requests")
                .push(request.method);
        }
        if writer
            .write_all(format!("{line}\n").as_bytes())
            .await
            .is_err()
        {
            break;
        }
    }
}

fn processor(mode: RequestDispatchMode) -> ConnectionProcessor {
    ConnectionProcessor::new_with_location_request(
        ExecServerRuntimePaths::new(
            std::env::current_exe().expect("test executable"),
            /*codex_linux_sandbox_exe*/ None,
        )
        .expect("runtime paths"),
        ExecServerTelemetry::default(),
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        mode,
        Err(io::Error::other(
            "connection fixture does not scan the real home",
        )),
    )
}

fn params(path: &PathUri, expected: &[u8], data: &[u8]) -> FsWriteFileIfUnchangedParams {
    FsWriteFileIfUnchangedParams {
        path: path.clone(),
        expected_data_base64: STANDARD.encode(expected),
        data_base64: STANDARD.encode(data),
        follow_symlinks: Some(false),
        sandbox: None,
    }
}

#[test_case(RequestDispatchMode::Inline, Phase::Compared; "inline_after_comparison")]
#[test_case(RequestDispatchMode::Inline, Phase::Blocking; "inline_inside_blocking_write")]
#[test_case(RequestDispatchMode::Inline, Phase::Settled; "inline_lost_reply_after_effect")]
#[test_case(RequestDispatchMode::Concurrent { max_concurrent_requests: ConcurrentRequestLimit::new(/*max_concurrent_requests*/ 2).expect("valid concurrency") }, Phase::Compared; "concurrent_after_comparison")]
#[test_case(RequestDispatchMode::Concurrent { max_concurrent_requests: ConcurrentRequestLimit::new(/*max_concurrent_requests*/ 2).expect("valid concurrency") }, Phase::Blocking; "concurrent_inside_blocking_write")]
#[test_case(RequestDispatchMode::Concurrent { max_concurrent_requests: ConcurrentRequestLimit::new(/*max_concurrent_requests*/ 2).expect("valid concurrency") }, Phase::Settled; "concurrent_lost_reply_after_effect")]
#[tokio::test]
async fn disconnect_keeps_effect_owned_and_never_resends(mode: RequestDispatchMode, phase: Phase) {
    let processor = processor(mode);
    let first = Connection::connect(processor.clone()).await;
    let second = Connection::connect(processor.clone()).await;
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical temporary root");
    let path = root.join("file");
    let uri = PathUri::from_host_native_path(&path).expect("path URI");
    std::fs::write(&path, b"before").expect("initial bytes");
    let mut pause = Pause::new(&path, phase);
    let client = first.client.clone();
    let request = params(&uri, b"before", b"first");
    let write = tokio::spawn(async move { client.fs_write_file_if_unchanged(request).await });
    pause.entered().await;
    if phase == Phase::Settled {
        assert_eq!(
            std::fs::read(&path).expect("committed before reply loss"),
            b"first"
        );
    }
    let requests = first.close().await;
    assert!(
        timeout(Duration::from_secs(/*secs*/ 10), write)
            .await
            .expect("lost reply reported")
            .expect("client task")
            .is_err()
    );
    // The first request/connection is gone, but its admitted effect still owns exclusion.
    assert_eq!(available_permits(), 0);
    let mut waiting = Pause::new(&path, Phase::Waiting);
    let client = second.client.clone();
    let request = params(&uri, b"first", b"second");
    let next = tokio::spawn(async move { client.fs_write_file_if_unchanged(request).await });
    waiting.entered().await;
    assert_eq!(available_permits(), 0);
    assert!(
        !next.is_finished(),
        "the second initialized connection waits behind the owned effect"
    );
    pause.release();
    assert!(
        timeout(Duration::from_secs(/*secs*/ 10), next)
            .await
            .expect("second connection progresses")
            .expect("second client task")
            .expect("second conditional response")
            .written
    );
    assert_eq!(std::fs::read(path).expect("ordered final bytes"), b"second");
    assert_eq!(
        requests
            .iter()
            .filter(|method| method.starts_with("fs/"))
            .cloned()
            .collect::<Vec<_>>(),
        vec![FS_WRITE_FILE_IF_UNCHANGED_METHOD]
    );
    let requests = second.close().await;
    assert_eq!(
        requests
            .iter()
            .filter(|method| method.starts_with("fs/"))
            .cloned()
            .collect::<Vec<_>>(),
        vec![FS_WRITE_FILE_IF_UNCHANGED_METHOD]
    );
    processor.shutdown().await;
}

#[tokio::test]
async fn registered_conditional_route_rejects_malformed_bytes_and_foreign_paths() {
    let processor = processor(RequestDispatchMode::Inline);
    let connection = Connection::connect(processor.clone()).await;
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical temporary root");
    let path = root.join("file");
    let uri = PathUri::from_host_native_path(&path).expect("path URI");
    std::fs::write(&path, b"before").expect("initial bytes");
    for field in ["expectedDataBase64", "dataBase64"] {
        let mut request = params(&uri, b"before", b"after");
        if field == "expectedDataBase64" {
            request.expected_data_base64 = "!".to_string();
        } else {
            request.data_base64 = "!".to_string();
        }
        let error = connection
            .client
            .fs_write_file_if_unchanged(request)
            .await
            .expect_err("invalid base64");
        assert!(
            error.to_string().contains(&format!("valid base64 {field}")),
            "{error}"
        );
    }
    #[cfg(not(windows))]
    let foreign = PathUri::parse("file:///C:/foreign/file").expect("foreign URI");
    #[cfg(windows)]
    let foreign = PathUri::parse("file:///foreign/file").expect("foreign URI");
    connection
        .client
        .fs_write_file_if_unchanged(params(&foreign, b"before", b"after"))
        .await
        .expect_err("native conversion fails closed");
    assert_eq!(std::fs::read(path).expect("unchanged bytes"), b"before");
    connection.close().await;
    processor.shutdown().await;
}

#[tokio::test]
async fn registered_conditional_route_requires_initialization() {
    let processor = processor(RequestDispatchMode::Inline);
    let server_processor = processor.clone();
    let (mut writer, server_reader) = duplex(/*max_buf_size*/ 65536);
    let (server_writer, reader) = duplex(/*max_buf_size*/ 65536);
    let server = tokio::spawn(async move {
        server_processor
            .run_connection(
                JsonRpcConnection::from_stdio(
                    server_reader,
                    server_writer,
                    "uninitialized conditional fixture".to_string(),
                ),
                ConnectionTransport::Stdio,
            )
            .await;
    });
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical temporary root");
    let path = root.join("file");
    std::fs::write(&path, b"before").expect("initial bytes");
    let request = serde_json::json!({"id": 1, "method": FS_WRITE_FILE_IF_UNCHANGED_METHOD,
        "params": params(&PathUri::from_host_native_path(&path).expect("URI"), b"before", b"after")});
    writer
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("request bytes");
    let mut reader = BufReader::new(reader).lines();
    let response = timeout(Duration::from_secs(/*secs*/ 10), reader.next_line())
        .await
        .expect("initialization error")
        .expect("response bytes")
        .expect("response line");
    let response: JSONRPCMessage = serde_json::from_str(&response).expect("JSON-RPC response");
    let JSONRPCMessage::Error(error) = response else {
        panic!("expected initialization error, got {response:?}");
    };
    assert!(
        error.error.message.contains("initializ"),
        "{}",
        error.error.message
    );
    assert_eq!(std::fs::read(path).expect("unchanged bytes"), b"before");
    drop(writer);
    drop(reader);
    timeout(Duration::from_secs(/*secs*/ 10), server)
        .await
        .expect("server disconnect")
        .expect("server task");
    processor.shutdown().await;
}
