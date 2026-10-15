use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use codex_core::config::Config;
use codex_core::config::ConfigBuilder;
use codex_core::config::ConfigOverrides;
use codex_core::config::LoaderOverrides;
use codex_extension_api::ConversationHistory;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistry;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ExtensionTurnItem;
use codex_extension_api::ThreadOriginator;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_extension_api::TurnItemEmissionFuture;
use codex_extension_api::TurnItemEmitter;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::TruncationPolicy;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;

#[derive(Clone, Debug)]
pub(super) struct RecordedRequest {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

pub(super) struct HttpFixture {
    pub url: String,
    server: MockServer,
}

impl HttpFixture {
    pub async fn new(responses: Vec<(u16, Value)>) -> Self {
        let server = MockServer::start().await;
        let expected_requests = responses.len() as u64;
        let responses = Mutex::new(VecDeque::from(responses));
        Mock::given(method("POST"))
            .respond_with(move |request: &Request| {
                assert!(request.body.len() <= 512 * 1024, "bounded fixture request");
                let (status, body) = responses
                    .lock()
                    .expect("fixture responses")
                    .pop_front()
                    .unwrap_or((
                        500,
                        json!({"error":{"message":"unexpected fixture request"}}),
                    ));
                ResponseTemplate::new(status)
                    .insert_header("x-codex-imagegen-request-id", "composition-image-request")
                    .insert_header("x-request-id", "composition-outer-request")
                    .set_body_json(body)
            })
            .expect(expected_requests)
            .mount(&server)
            .await;
        Self {
            url: server.uri(),
            server,
        }
    }

    pub async fn requests(&self) -> Vec<RecordedRequest> {
        self.server
            .received_requests()
            .await
            .expect("request recording enabled")
            .into_iter()
            .map(|request| RecordedRequest {
                path: request.url.path().to_string(),
                headers: request
                    .headers
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.as_str().to_string(),
                            value.to_str().expect("fixture header").to_string(),
                        )
                    })
                    .collect(),
                body: request.body_json().expect("JSON request body"),
            })
            .collect()
    }
}

pub(super) fn provider(url: &str, name: &str, wire_api: WireApi) -> ModelProviderInfo {
    ModelProviderInfo {
        name: name.to_string(),
        base_url: Some(url.to_string()),
        wire_api,
        experimental_bearer_token: Some("fixture-provider".into()),
        request_max_retries: Some(0),
        ..ModelProviderInfo::default()
    }
}

pub(super) struct Harness {
    pub config: Config,
    pub registry: ExtensionRegistry<Config>,
    pub session: ExtensionData,
    pub thread: ExtensionData,
    _directory: TempDir,
}

impl Harness {
    pub async fn new(provider: ModelProviderInfo) -> Self {
        let directory = tempfile::tempdir().expect("fixture directory");
        let mut config = ConfigBuilder::default()
            .loader_overrides(LoaderOverrides {
                ignore_user_config: true,
                ignore_project_config: true,
                ..LoaderOverrides::without_managed_config_for_tests()
            })
            .codex_home(directory.path().to_path_buf())
            .harness_overrides(ConfigOverrides {
                cwd: Some(directory.path().to_path_buf()),
                ..ConfigOverrides::default()
            })
            .build()
            .await
            .expect("isolated extension config");
        config.model_provider = provider;
        config.application_network_policy = Default::default();
        let mut builder = ExtensionRegistryBuilder::new();
        crate::install(
            &mut builder,
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture-auth")),
            |config| Some(config.codex_home.join("artifacts")),
        );
        let harness = Self {
            config,
            registry: builder.build(),
            session: ExtensionData::new("session-composition"),
            thread: ExtensionData::new("thread-composition"),
            _directory: directory,
        };
        harness
            .thread
            .insert(ThreadOriginator("composition-originator".to_string()));
        assert!(
            harness.tools().is_empty(),
            "tools require initialized thread configuration"
        );
        for contributor in harness.registry.thread_lifecycle_contributors() {
            contributor
                .on_thread_start(ThreadStartInput {
                    config: &harness.config,
                    session_source: &SessionSource::Cli,
                    persistent_thread_state_available: true,
                    environments: &[],
                    mcp_resource_client: None,
                    extension_metrics: None,
                    session_store: &harness.session,
                    thread_store: &harness.thread,
                })
                .await;
        }
        harness
    }

    pub fn tools(&self) -> Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>> {
        self.registry
            .tool_contributors()
            .iter()
            .flat_map(|contributor| contributor.tools(&self.session, &self.thread))
            .collect()
    }

    pub fn tool(&self) -> Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>> {
        let mut tools = self.tools();
        assert_eq!(tools.len(), 1);
        tools.pop().expect("image tool")
    }

    pub fn refresh(&mut self, config: Config) {
        for contributor in self.registry.config_contributors() {
            contributor.on_config_changed(&self.session, &self.thread, &self.config, &config);
        }
        self.config = config;
    }
}

#[derive(Default)]
pub(super) struct Items(pub Mutex<Vec<(&'static str, ExtensionTurnItem)>>);

impl TurnItemEmitter for Items {
    fn emit_started<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        Box::pin(async move {
            self.0.lock().expect("items").push(("started", item));
        })
    }

    fn emit_completed<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        Box::pin(async move {
            self.0.lock().expect("items").push(("completed", item));
        })
    }
}

pub(super) fn call(
    id: &str,
    arguments: Value,
    history: Vec<ResponseItem>,
    items: Arc<Items>,
    source: ToolCallSource,
) -> ToolCall<'static> {
    ToolCall {
        turn_id: format!("turn-{id}"),
        call_id: id.to_string(),
        tool_name: ToolName::namespaced("image_gen", "imagegen"),
        model: "fixture-grok-model".to_string(),
        codex_turn_metadata: None,
        truncation_policy: TruncationPolicy::Bytes(10000),
        source,
        conversation_history: ConversationHistory::new(history),
        turn_item_emitter: items,
        environments: Vec::new(),
        payload: ToolPayload::Function {
            arguments: arguments.to_string(),
        },
    }
}
