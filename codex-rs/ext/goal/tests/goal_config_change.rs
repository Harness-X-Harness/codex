use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;

use anyhow::Context;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use codex_analytics::AnalyticsEventsClient;
use codex_extension_api::EngineOccupant;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolExecutor;
use codex_extension_api::engine_slot;
use codex_goal_extension::GoalExtensionConfig;
use codex_goal_extension::GoalObjectiveUpdate;
use codex_goal_extension::GoalPolicy;
use codex_goal_extension::GoalRuntimeHandle;
use codex_goal_extension::GoalService;
use codex_goal_extension::GoalSetRequest;
use codex_goal_extension::GoalTokenBudgetUpdate;
use codex_goal_extension::install_with_backend;
use codex_protocol::ThreadId;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadGoalStatus;

#[tokio::test]
async fn internal_thread_stays_host_evaluate_disabled_after_config_reload() -> anyhow::Result<()> {
    let runtime = test_runtime().await?;
    let thread_id = test_thread_id()?;
    seed_thread_metadata(runtime.as_ref(), thread_id).await?;
    let (registry, session_store, thread_store) = started_goal_with_service(
        runtime,
        Arc::new(GoalService::new()),
        thread_id,
        SessionSource::Internal(InternalSessionSource::GoalSkeptic),
        host_config(),
    )
    .await;

    let handle = runtime_handle(&thread_store)?;
    assert!(!handle.is_enabled());
    assert!(tool_names(&registry, &session_store, &thread_store).is_empty());

    apply_config(&registry, &session_store, &thread_store, host_config());
    assert!(!handle.is_enabled());
    assert_eq!(handle.policy(), GoalPolicy::host_evaluate());
    assert!(tool_names(&registry, &session_store, &thread_store).is_empty());
    assert_eq!(engine_slot(&thread_store).occupant(), None);
    Ok(())
}

#[tokio::test]
async fn host_evaluate_off_releases_goal_how_and_exposes_stock_update_goal() -> anyhow::Result<()> {
    let runtime = test_runtime().await?;
    let thread_id = test_thread_id()?;
    seed_thread_metadata(runtime.as_ref(), thread_id).await?;
    let goal_service = Arc::new(GoalService::new());
    let (registry, session_store, thread_store) = started_goal_with_service(
        Arc::clone(&runtime),
        Arc::clone(&goal_service),
        thread_id,
        SessionSource::Cli,
        host_config(),
    )
    .await;
    set_active_goal(runtime.as_ref(), &goal_service, thread_id).await?;
    let slot = engine_slot(&thread_store);
    assert!(slot.try_claim(EngineOccupant::GoalHow));

    apply_config(&registry, &session_store, &thread_store, stock_config());
    let handle = runtime_handle(&thread_store)?;
    assert!(handle.is_enabled());
    assert_eq!(handle.policy(), GoalPolicy::model_commit());
    assert_eq!(slot.occupant(), None);
    assert!(
        tool_names(&registry, &session_store, &thread_store).contains(&"update_goal".to_string()),
        "stock Goals must expose update_goal after goal_host turns off"
    );
    let persisted = runtime
        .thread_goals()
        .get_thread_goal(thread_id)
        .await?
        .context("persisted goal")?;
    assert_eq!(persisted.status, codex_state::ThreadGoalStatus::Active);
    Ok(())
}

#[tokio::test]
async fn reenabling_host_evaluate_claims_the_slot_only_when_it_is_free() -> anyhow::Result<()> {
    let runtime = test_runtime().await?;
    let thread_id = test_thread_id()?;
    seed_thread_metadata(runtime.as_ref(), thread_id).await?;
    let goal_service = Arc::new(GoalService::new());
    let (registry, session_store, thread_store) = started_goal_with_service(
        Arc::clone(&runtime),
        Arc::clone(&goal_service),
        thread_id,
        SessionSource::Cli,
        stock_config(),
    )
    .await;
    set_active_goal(runtime.as_ref(), &goal_service, thread_id).await?;
    let slot = engine_slot(&thread_store);
    assert!(slot.try_claim(EngineOccupant::Workflow));

    apply_config(&registry, &session_store, &thread_store, host_config());
    let held = std::time::Instant::now() + Duration::from_millis(80);
    while std::time::Instant::now() < held {
        assert_eq!(slot.occupant(), Some(EngineOccupant::Workflow));
        tokio::task::yield_now().await;
    }
    assert_eq!(
        runtime_handle(&thread_store)?.policy(),
        GoalPolicy::host_evaluate()
    );
    assert!(
        !tool_names(&registry, &session_store, &thread_store).contains(&"update_goal".to_string()),
        "host evaluate must hide update_goal"
    );

    assert!(slot.release(EngineOccupant::Workflow));
    apply_config(&registry, &session_store, &thread_store, host_config());
    wait_until_goal_how(&thread_store).await;
    Ok(())
}

fn host_config() -> GoalExtensionConfig {
    GoalExtensionConfig {
        enabled: true,
        max_goal_token_budget: None,
        policy: GoalPolicy::host_evaluate(),
    }
}

fn stock_config() -> GoalExtensionConfig {
    GoalExtensionConfig {
        enabled: true,
        max_goal_token_budget: None,
        policy: GoalPolicy::model_commit(),
    }
}

async fn started_goal_with_service(
    runtime: Arc<codex_state::StateRuntime>,
    goal_service: Arc<GoalService>,
    thread_id: ThreadId,
    session_source: SessionSource,
    start_config: GoalExtensionConfig,
) -> (
    codex_extension_api::ExtensionRegistry<GoalExtensionConfig>,
    ExtensionData,
    ExtensionData,
) {
    let mut builder = ExtensionRegistryBuilder::<GoalExtensionConfig>::new();
    install_with_backend(
        &mut builder,
        runtime,
        AnalyticsEventsClient::disabled(),
        /*metrics_client*/ None,
        Weak::new(),
        goal_service,
        GoalExtensionConfig::clone,
    );
    let registry = builder.build();
    let session_store = ExtensionData::new(thread_id.to_string());
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
    registry: &codex_extension_api::ExtensionRegistry<GoalExtensionConfig>,
    session_store: &ExtensionData,
    thread_store: &ExtensionData,
    next: GoalExtensionConfig,
) {
    let previous = thread_store
        .get::<GoalExtensionConfig>()
        .map(|config| GoalExtensionConfig::clone(&config))
        .unwrap_or_else(stock_config);
    for contributor in registry.config_contributors() {
        contributor.on_config_changed(session_store, thread_store, &previous, &next);
    }
}

fn runtime_handle(thread_store: &ExtensionData) -> anyhow::Result<Arc<GoalRuntimeHandle>> {
    thread_store
        .get::<GoalRuntimeHandle>()
        .context("goal runtime should be stored on thread start")
}

fn tool_names(
    registry: &codex_extension_api::ExtensionRegistry<GoalExtensionConfig>,
    session_store: &ExtensionData,
    thread_store: &ExtensionData,
) -> Vec<String> {
    registry
        .tool_contributors()
        .iter()
        .flat_map(|contributor| contributor.tools(session_store, thread_store))
        .map(|tool| tool.tool_name().name)
        .collect()
}

async fn set_active_goal(
    runtime: &codex_state::StateRuntime,
    goal_service: &GoalService,
    thread_id: ThreadId,
) -> anyhow::Result<()> {
    let outcome = goal_service
        .set_thread_goal(
            runtime,
            GoalSetRequest {
                thread_id,
                objective: GoalObjectiveUpdate::Set("keep host evaluate across config reload"),
                status: None,
                token_budget: GoalTokenBudgetUpdate::Keep,
                max_goal_token_budget: None,
            },
        )
        .await?;
    assert_eq!(outcome.goal.status, ThreadGoalStatus::Active);
    Ok(())
}

async fn wait_until_goal_how(thread_store: &ExtensionData) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let occupant = engine_slot(thread_store).occupant();
        if occupant == Some(EngineOccupant::GoalHow) {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("engine occupant stayed {occupant:?}, expected GoalHow");
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn test_runtime() -> anyhow::Result<Arc<codex_state::StateRuntime>> {
    let tempdir = TempDir::new()?;
    codex_state::StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(tempdir.keep().as_path().abs()),
        "test-provider".to_string(),
    )
    .await
}

fn test_thread_id() -> anyhow::Result<ThreadId> {
    ThreadId::from_string("22222222-2222-4222-8222-222222222222").map_err(anyhow::Error::msg)
}

async fn seed_thread_metadata(
    runtime: &codex_state::StateRuntime,
    thread_id: ThreadId,
) -> anyhow::Result<()> {
    let builder = codex_state::ThreadMetadataBuilder::new(
        thread_id,
        runtime
            .sqlite()
            .home()
            .join(format!("rollout-{thread_id}.jsonl")),
        chrono::Utc::now(),
        SessionSource::Cli,
    );
    runtime.upsert_thread(&builder.build("test-provider")).await
}
