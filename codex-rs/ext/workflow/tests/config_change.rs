use std::sync::Arc;
use std::time::Duration;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_extension_api::EngineOccupant;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::HostIdleHold;
use codex_extension_api::ThreadIdleCause;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::engine_slot;
use codex_protocol::ThreadId;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use codex_workflow_extension::WorkflowExtensionConfig;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::WorkflowStatus;
use codex_workflow_extension::install;
use codex_workflow_extension::persist_workflow_document;

const ASK_THEN_COMPLETE: &str = r#"ask("next"); complete();"#;

#[tokio::test]
async fn internal_thread_stays_workflow_disabled_after_config_reload() {
    let dir = TempDir::new().expect("tempdir");
    let service = Arc::new(WorkflowService::new(
        dir.path().to_path_buf(),
        std::sync::Weak::new(),
    ));
    let (registry, session_store, thread_store) = started_workflow(
        Arc::clone(&service),
        ThreadId::from_u128(2341),
        SessionSource::Internal(InternalSessionSource::MemoryConsolidation),
        WorkflowExtensionConfig { enabled: true },
    )
    .await;

    assert_eq!(
        thread_store
            .get::<WorkflowExtensionConfig>()
            .map(|config| config.enabled),
        Some(false)
    );

    apply_config(
        &registry,
        &session_store,
        &thread_store,
        WorkflowExtensionConfig { enabled: true },
    );
    assert_eq!(
        thread_store
            .get::<WorkflowExtensionConfig>()
            .map(|config| config.enabled),
        Some(false)
    );
}

#[tokio::test]
async fn disabling_goal_host_pauses_active_workflow_and_releases_the_engine() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(2342);
    let service = Arc::new(WorkflowService::new(
        dir.path().to_path_buf(),
        std::sync::Weak::new(),
    ));
    service
        .start_run(thread_id, ASK_THEN_COMPLETE)
        .await
        .expect("start");
    let (registry, session_store, thread_store) = started_workflow(
        Arc::clone(&service),
        thread_id,
        SessionSource::Cli,
        WorkflowExtensionConfig { enabled: true },
    )
    .await;
    let slot = engine_slot(&thread_store);
    assert!(slot.try_claim(EngineOccupant::Workflow));
    thread_store.insert(HostIdleHold);

    apply_config(
        &registry,
        &session_store,
        &thread_store,
        WorkflowExtensionConfig { enabled: false },
    );
    assert!(thread_store.get::<HostIdleHold>().is_none());
    wait_until_status(&service, thread_id, WorkflowStatus::Paused).await;
    assert_eq!(slot.occupant(), None);
    assert_eq!(
        thread_store
            .get::<WorkflowExtensionConfig>()
            .map(|config| config.enabled),
        Some(false)
    );

    emit_idle(&registry, &session_store, &thread_store).await;
    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(run.status, WorkflowStatus::Paused);
}

#[tokio::test]
async fn disabling_goal_host_pauses_waiting_workflow() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(2343);
    let queued =
        codex_workflow_extension::WorkflowRun::queue(thread_id, ASK_THEN_COMPLETE).expect("queue");
    persist_workflow_document(
        dir.path(),
        &thread_id.to_string(),
        &serde_json::to_vec_pretty(&queued).expect("serialize"),
    )
    .expect("persist");
    let service = Arc::new(WorkflowService::new(
        dir.path().to_path_buf(),
        std::sync::Weak::new(),
    ));
    let (registry, session_store, thread_store) = started_workflow(
        Arc::clone(&service),
        thread_id,
        SessionSource::Cli,
        WorkflowExtensionConfig { enabled: true },
    )
    .await;
    assert_eq!(
        service
            .get_run(thread_id)
            .await
            .expect("get")
            .expect("run")
            .status,
        WorkflowStatus::Waiting
    );

    apply_config(
        &registry,
        &session_store,
        &thread_store,
        WorkflowExtensionConfig { enabled: false },
    );
    wait_until_status(&service, thread_id, WorkflowStatus::Paused).await;
}

#[tokio::test]
async fn reenabling_goal_host_does_not_auto_resume_a_paused_workflow() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(2344);
    let service = Arc::new(WorkflowService::new(
        dir.path().to_path_buf(),
        std::sync::Weak::new(),
    ));
    service
        .start_run(thread_id, ASK_THEN_COMPLETE)
        .await
        .expect("start");
    let (registry, session_store, thread_store) = started_workflow(
        Arc::clone(&service),
        thread_id,
        SessionSource::Cli,
        WorkflowExtensionConfig { enabled: true },
    )
    .await;

    apply_config(
        &registry,
        &session_store,
        &thread_store,
        WorkflowExtensionConfig { enabled: false },
    );
    wait_until_status(&service, thread_id, WorkflowStatus::Paused).await;

    apply_config(
        &registry,
        &session_store,
        &thread_store,
        WorkflowExtensionConfig { enabled: true },
    );
    assert_eq!(
        thread_store
            .get::<WorkflowExtensionConfig>()
            .map(|config| config.enabled),
        Some(true)
    );
    emit_idle(&registry, &session_store, &thread_store).await;
    let paused = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(paused.status, WorkflowStatus::Paused);

    let resumed = service.resume_run(thread_id).await.expect("resume");
    assert_eq!(resumed.status, WorkflowStatus::Active);
}

async fn started_workflow(
    service: Arc<WorkflowService>,
    thread_id: ThreadId,
    session_source: SessionSource,
    start_config: WorkflowExtensionConfig,
) -> (
    codex_extension_api::ExtensionRegistry<WorkflowExtensionConfig>,
    ExtensionData,
    ExtensionData,
) {
    let mut builder = ExtensionRegistryBuilder::<WorkflowExtensionConfig>::new();
    install(&mut builder, service, |config| *config);
    let registry = builder.build();
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new(thread_id.to_string());
    for contributor in registry.thread_lifecycle_contributors() {
        contributor
            .on_thread_start(ThreadStartInput {
                config: &start_config,
                session_source: &session_source,
                persistent_thread_state_available: true,
                environments: &[],
                mcp_resource_client: None,
                extension_metrics: None,
                session_store: &session_store,
                thread_store: &thread_store,
            })
            .await;
    }
    (registry, session_store, thread_store)
}

fn apply_config(
    registry: &codex_extension_api::ExtensionRegistry<WorkflowExtensionConfig>,
    session_store: &ExtensionData,
    thread_store: &ExtensionData,
    next: WorkflowExtensionConfig,
) {
    let previous = thread_store
        .get::<WorkflowExtensionConfig>()
        .map(|config| *config)
        .unwrap_or(WorkflowExtensionConfig { enabled: true });
    for contributor in registry.config_contributors() {
        contributor.on_config_changed(session_store, thread_store, &previous, &next);
    }
}

async fn emit_idle(
    registry: &codex_extension_api::ExtensionRegistry<WorkflowExtensionConfig>,
    session_store: &ExtensionData,
    thread_store: &ExtensionData,
) {
    for contributor in registry.thread_lifecycle_contributors() {
        contributor
            .on_thread_idle(ThreadIdleInput {
                cause: ThreadIdleCause::Completed,
                session_store,
                thread_store,
            })
            .await;
    }
}

async fn wait_until_status(
    service: &WorkflowService,
    thread_id: ThreadId,
    expected: WorkflowStatus,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let status = service
            .get_run(thread_id)
            .await
            .expect("get")
            .map(|run| run.status);
        if status == Some(expected) {
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!("workflow status stayed {status:?}, expected {expected:?}");
        }
        tokio::task::yield_now().await;
    }
}
