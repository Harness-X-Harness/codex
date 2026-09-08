use std::sync::Arc;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_extension_api::EngineOccupant;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::HostIdleHold;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::engine_slot;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_workflow_extension::WorkflowExtensionConfig;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::WorkflowStatus;
use codex_workflow_extension::install;
use codex_workflow_extension::load_workflow_document;

#[tokio::test]
async fn on_thread_stop_keeps_persist_and_releases_the_store() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(2360);
    let service = Arc::new(WorkflowService::new(
        dir.path().to_path_buf(),
        std::sync::Weak::new(),
    ));
    let started = service
        .start_run(thread_id, r#"ask("next"); complete();"#)
        .await
        .expect("start");
    assert_eq!(started.status, WorkflowStatus::Active);

    let mut builder = ExtensionRegistryBuilder::<WorkflowExtensionConfig>::new();
    install(&mut builder, Arc::clone(&service), |config| *config);
    let registry = builder.build();
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new(thread_id.to_string());
    let start_config = WorkflowExtensionConfig { enabled: true };
    for contributor in registry.thread_lifecycle_contributors() {
        contributor
            .on_thread_start(ThreadStartInput {
                config: &start_config,
                session_source: &SessionSource::Cli,
                persistent_thread_state_available: true,
                environments: &[],
                mcp_resource_client: None,
                extension_metrics: None,
                session_store: &session_store,
                thread_store: &thread_store,
            })
            .await;
    }
    let slot = engine_slot(&thread_store);
    assert!(slot.try_claim(EngineOccupant::Workflow));
    thread_store.insert(HostIdleHold);

    for contributor in registry.thread_lifecycle_contributors() {
        contributor
            .on_thread_stop(ThreadStopInput {
                session_store: &session_store,
                thread_store: &thread_store,
            })
            .await;
    }

    assert!(thread_store.get::<HostIdleHold>().is_none());
    assert_eq!(slot.occupant(), None);
    assert!(
        load_workflow_document(dir.path(), &thread_id.to_string())
            .expect("persist after stop")
            .is_some()
    );
    let reloaded = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(reloaded.status, WorkflowStatus::Active);
    assert_eq!(reloaded.run_id, started.run_id);
}

#[tokio::test]
async fn on_thread_stop_keeps_persist_when_goal_host_is_off() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(2364);
    let service = Arc::new(WorkflowService::new(
        dir.path().to_path_buf(),
        std::sync::Weak::new(),
    ));
    service
        .start_run(thread_id, r#"ask("next"); complete();"#)
        .await
        .expect("start");

    let mut builder = ExtensionRegistryBuilder::<WorkflowExtensionConfig>::new();
    install(&mut builder, Arc::clone(&service), |config| *config);
    let registry = builder.build();
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new(thread_id.to_string());
    let start_config = WorkflowExtensionConfig { enabled: false };
    for contributor in registry.thread_lifecycle_contributors() {
        contributor
            .on_thread_start(ThreadStartInput {
                config: &start_config,
                session_source: &SessionSource::Cli,
                persistent_thread_state_available: true,
                environments: &[],
                mcp_resource_client: None,
                extension_metrics: None,
                session_store: &session_store,
                thread_store: &thread_store,
            })
            .await;
    }
    for contributor in registry.thread_lifecycle_contributors() {
        contributor
            .on_thread_stop(ThreadStopInput {
                session_store: &session_store,
                thread_store: &thread_store,
            })
            .await;
    }
    assert!(
        load_workflow_document(dir.path(), &thread_id.to_string())
            .expect("persist after stop")
            .is_some()
    );
    let reloaded = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(reloaded.status, WorkflowStatus::Active);
}
