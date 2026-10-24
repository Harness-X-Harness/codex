use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use codex_utils_path_uri::PathUri;
use codex_utils_pty::Command;
use pretty_assertions::assert_eq;
use tokio::io::AsyncRead;
use tokio::io::ReadBuf;
use tokio::sync::oneshot;
use tokio::time::timeout;

use super::collect_helper_pipes;
use crate::ConditionalWriteResult;
use crate::ExecutorFileSystem;
use crate::LocalFileSystem;
use crate::WriteFileOptions;
use crate::file_system_mutation;
use crate::file_system_mutation::test_support::available_permits;

struct ReadFailure<R> {
    reader: R,
    received_bytes: bool,
    failed: Option<oneshot::Sender<()>>,
}

impl<R: AsyncRead + Unpin> AsyncRead for ReadFailure<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.received_bytes {
            if let Some(failed) = this.failed.take() {
                let _ = failed.send(());
            }
            return Poll::Ready(Err(io::Error::other("controlled helper pipe failure")));
        }
        let before = buffer.filled().len();
        let result = Pin::new(&mut this.reader).poll_read(context, buffer);
        this.received_bytes |= buffer.filled().len() > before;
        result
    }
}

struct ReleaseOnDrop(PathBuf);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, b"release");
    }
}

#[tokio::test]
async fn helper_pipe_failure_does_not_release_a_surviving_effect() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("file");
    let release = directory.path().join("release");
    let _release_on_drop = ReleaseOnDrop(release.clone());
    std::fs::write(&path, b"before").expect("initial bytes");
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("printf ready; while [ ! -f \"$1\" ]; do sleep 0.01; done; printf helper > \"$2\"")
        .arg("helper-effect-fixture")
        .arg(&release)
        .arg(&path);
    command.envs(std::env::vars_os());
    let (failed_tx, failed_rx) = oneshot::channel();
    let (settled_tx, settled_rx) = oneshot::channel();
    let caller = tokio::spawn(file_system_mutation::run(async move {
        let mut child = command.spawn().expect("real helper process");
        let mut stdout = ReadFailure {
            reader: child.stdout.take().expect("helper stdout"),
            received_bytes: false,
            failed: Some(failed_tx),
        };
        let mut stderr = child.stderr.take();
        let (exited, result) =
            collect_helper_pipes(&mut child, Some(&mut stdout), stderr.as_mut()).await;
        assert!(
            exited,
            "the original pipe failure must not cancel child.wait"
        );
        let error = result.expect_err("original pipe error retained after exit");
        let _ = settled_tx.send(error.kind());
        Err::<(), _>(error)
    }));
    timeout(Duration::from_secs(/*secs*/ 10), failed_rx)
        .await
        .expect("pipe fault reached")
        .expect("pipe fault signal");
    caller.abort();
    assert!(caller.await.expect_err("cancelled waiter").is_cancelled());
    assert_eq!(available_permits(), 0);
    assert_eq!(
        std::fs::read(&path).expect("effect still waiting"),
        b"before"
    );
    let fs: Arc<dyn ExecutorFileSystem> = Arc::new(LocalFileSystem::unsandboxed());
    let uri = PathUri::from_host_native_path(&path).expect("path URI");
    let mut next = fs.write_file_if_unchanged(
        &uri,
        b"helper".to_vec(),
        b"after".to_vec(),
        WriteFileOptions::default(),
        /*sandbox*/ None,
    );
    assert!(futures::poll!(next.as_mut()).is_pending());
    std::fs::write(release, b"release").expect("allow actual helper effect");
    assert_eq!(
        timeout(Duration::from_secs(/*secs*/ 10), next)
            .await
            .expect("successor proceeds after exit")
            .expect("conditional result"),
        ConditionalWriteResult::Written
    );
    assert_eq!(
        settled_rx.await.expect("settled error"),
        io::ErrorKind::Other
    );
    assert_eq!(std::fs::read(path).expect("ordered final bytes"), b"after");
}
