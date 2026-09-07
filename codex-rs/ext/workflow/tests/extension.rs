use std::sync::Arc;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_core::TurnStartOptions;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::TurnStopInput;
use codex_protocol::ThreadId;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_workflow_extension::WorkflowExtensionConfig;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::WorkflowStatus;
use codex_workflow_extension::install;

async fn persist_started_yield(
    dir: &TempDir,
    thread_id: ThreadId,
    source: &str,
) -> WorkflowService {
    let first = WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new());
    first.start_run(thread_id, source).await.expect("start");
    let mut run = first.get_run(thread_id).await.expect("get").expect("run");
    run.mark_pending_yield_started();
    std::fs::write(
        dir.path().join(format!("{thread_id}.json")),
        serde_json::to_vec_pretty(&run).expect("encode"),
    )
    .expect("write");
    WorkflowService::new(dir.path().to_path_buf(), std::sync::Weak::new())
}

async fn stop_workflow_turn(
    registry: &codex_extension_api::ExtensionRegistry<()>,
    thread_id: ThreadId,
    turn_store: &ExtensionData,
) {
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new(thread_id.to_string());
    thread_store.insert(WorkflowExtensionConfig { enabled: true });
    for contributor in registry.turn_lifecycle_contributors() {
        contributor
            .on_turn_stop(TurnStopInput {
                session_store: &session_store,
                thread_store: &thread_store,
                turn_store,
            })
            .await;
    }
}

fn workflow_turn_store() -> ExtensionData {
    let turn_store = ExtensionData::new("turn-1");
    turn_store.insert(TurnStartOptions {
        turn_trigger: Some("workflow".to_string()),
        ..Default::default()
    });
    turn_store
}

#[tokio::test]
async fn successful_workflow_turn_uses_turn_local_assistant_text() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(40);
    let service = Arc::new(
        persist_started_yield(
            &dir,
            thread_id,
            r#"
                let r = agent("Say ok.");
                if r.ok && r.text == "ok" { complete(); } else { ask("wrong reply"); }
            "#,
        )
        .await,
    );
    let mut builder = ExtensionRegistryBuilder::<()>::new();
    install(&mut builder, Arc::clone(&service), |_| {
        WorkflowExtensionConfig { enabled: true }
    });
    let registry = builder.build();
    let turn_store = workflow_turn_store();
    let mut item = TurnItem::AgentMessage(AgentMessageItem {
        id: "msg-1".to_string(),
        content: vec![AgentMessageContent::Text {
            text: "ok".to_string(),
        }],
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    });
    for contributor in registry.turn_item_contributors() {
        contributor
            .contribute(&ExtensionData::new("thread"), &turn_store, &mut item)
            .await
            .expect("contribute");
    }
    stop_workflow_turn(&registry, thread_id, &turn_store).await;
    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(run.status, WorkflowStatus::Complete);
}

#[tokio::test]
async fn successful_workflow_turn_without_assistant_item_keeps_empty_text() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = ThreadId::from_u128(41);
    let service = Arc::new(
        persist_started_yield(
            &dir,
            thread_id,
            r#"
                let r = agent("Say ok.");
                if r.ok && r.text == "" { complete(); } else { ask("wrong reply"); }
            "#,
        )
        .await,
    );
    let mut builder = ExtensionRegistryBuilder::<()>::new();
    install(&mut builder, Arc::clone(&service), |_| {
        WorkflowExtensionConfig { enabled: true }
    });
    let registry = builder.build();
    stop_workflow_turn(&registry, thread_id, &workflow_turn_store()).await;
    let run = service.get_run(thread_id).await.expect("get").expect("run");
    assert_eq!(run.status, WorkflowStatus::Complete);
}
