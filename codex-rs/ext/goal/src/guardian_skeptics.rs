//! Production [`GoalSkepticPanel`] backed by Guardian internal sessions.
//!
//! Each skeptic is a fresh [`InternalSessionSource::GoalSkeptic`] session started
//! through the host-injected [`InternalSessionSpawner`]. This is not worker
//! `spawn_agent`.

use std::collections::HashMap;
use std::sync::Weak;
use std::time::Duration;

use codex_core::NewThread;
use codex_core::StartIfIdleSubmission;
use codex_core::StartThreadOptions;
use codex_core::ThreadManager;
use codex_core::TurnInputRequest;
use codex_core::TurnStartOptions;
use codex_core::config::Config;
use codex_core::config::Constrained;
use codex_extension_api::InternalSessionSpawner;
use codex_features::Feature;
use codex_protocol::error::CodexErr;
use codex_protocol::models::BaseInstructionsProvenance;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadSource;
use codex_protocol::user_input::UserInput;
use futures::future::join_all;

use crate::evaluator_prompt::goal_evaluator_evidence;
use crate::host_verify::GoalSkepticError;
use crate::host_verify::GoalSkepticPanel;
use crate::host_verify::GoalSkepticPanelFuture;
use crate::host_verify::GoalSkepticPanelInput;
use crate::host_verify::GoalSkepticPanelVerdict;
use crate::host_verify::aggregate_skeptic_votes;
use crate::host_verify::parse_goal_skeptic_vote;
use crate::skeptic_prompt::build_goal_skeptic_user_payload;
use crate::skeptic_prompt::clamp_host_skeptic_count;
use crate::skeptic_prompt::goal_skeptic_output_schema;
use crate::skeptic_prompt::skeptic_system_prompt;

const SKEPTIC_TURN_TIMEOUT: Duration = Duration::from_secs(120);
const SKEPTIC_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// Host-owned skeptic panel that spawns Guardian internal sessions.
pub struct GuardianGoalSkepticPanel<S> {
    thread_manager: Weak<ThreadManager>,
    spawner: S,
}

impl<S> GuardianGoalSkepticPanel<S> {
    pub fn new(thread_manager: Weak<ThreadManager>, spawner: S) -> Self {
        Self {
            thread_manager,
            spawner,
        }
    }
}

impl<S> GoalSkepticPanel for GuardianGoalSkepticPanel<S>
where
    S: InternalSessionSpawner<StartThreadOptions, Spawned = NewThread, Error = CodexErr>,
{
    fn verify(&self, input: GoalSkepticPanelInput) -> GoalSkepticPanelFuture<'_> {
        Box::pin(async move { self.verify_inner(input).await })
    }
}

impl<S> GuardianGoalSkepticPanel<S>
where
    S: InternalSessionSpawner<StartThreadOptions, Spawned = NewThread, Error = CodexErr>,
{
    async fn verify_inner(
        &self,
        input: GoalSkepticPanelInput,
    ) -> Result<GoalSkepticPanelVerdict, GoalSkepticError> {
        let thread_manager = self.thread_manager.upgrade().ok_or_else(|| {
            GoalSkepticError::Failed("thread manager dropped before host skeptics".into())
        })?;
        let parent = thread_manager
            .get_thread(input.thread_id)
            .await
            .map_err(|error| GoalSkepticError::Failed(error.to_string()))?;
        let history = parent
            .load_history(/*include_archived*/ false)
            .await
            .map_err(|error| GoalSkepticError::Failed(error.to_string()))?;
        let evidence = goal_evaluator_evidence(&history.items);
        let parent_config = parent.config().await;
        let config = isolated_skeptic_config(parent_config.as_ref())?;
        let environments = parent.environment_selections().await;
        let count = clamp_host_skeptic_count(input.count);
        let votes = join_all((0..count).map(|skeptic_index| {
            let payload = build_goal_skeptic_user_payload(
                skeptic_index,
                count,
                &input.objective,
                &input.candidate_next_step,
                &evidence.transcript,
                evidence.plan.as_deref(),
            );
            run_one_skeptic(
                &self.spawner,
                thread_manager.as_ref(),
                input.thread_id,
                config.clone(),
                environments.clone(),
                payload,
            )
        }))
        .await;
        let votes = votes.into_iter().collect::<Result<Vec<_>, _>>()?;
        aggregate_skeptic_votes(&votes)
    }
}

async fn run_one_skeptic<S>(
    spawner: &S,
    thread_manager: &ThreadManager,
    parent_thread_id: codex_protocol::ThreadId,
    config: Config,
    environments: Vec<codex_protocol::protocol::TurnEnvironmentSelection>,
    payload: String,
) -> Result<crate::host_verify::GoalSkepticVote, GoalSkepticError>
where
    S: InternalSessionSpawner<StartThreadOptions, Spawned = NewThread, Error = CodexErr>,
{
    let options = StartThreadOptions {
        session_source: Some(SessionSource::Internal(InternalSessionSource::GoalSkeptic)),
        thread_source: Some(ThreadSource::Feature("goal_skeptic".to_string())),
        environments: Some(environments),
        ..StartThreadOptions::new(config)
    };
    let spawned = spawner
        .spawn_internal_session(parent_thread_id, options)
        .await
        .map_err(|error| GoalSkepticError::Failed(error.to_string()))?;
    let result = collect_skeptic_vote(&spawned.thread, payload).await;
    shutdown_skeptic(thread_manager, spawned).await;
    result
}

async fn collect_skeptic_vote(
    thread: &codex_core::CodexThread,
    payload: String,
) -> Result<crate::host_verify::GoalSkepticVote, GoalSkepticError> {
    let request = TurnInputRequest::user_input(vec![UserInput::Text {
        text: payload,
        text_elements: Vec::new(),
    }])
    .on_start(TurnStartOptions {
        final_output_json_schema: Some(goal_skeptic_output_schema()),
        ..TurnStartOptions::default()
    });
    match thread.start_turn_if_idle(request).await {
        Ok(StartIfIdleSubmission::Started { .. }) => {}
        Ok(submission) => {
            return Err(GoalSkepticError::Failed(format!(
                "host skeptic turn was not started: {submission:?}"
            )));
        }
        Err(error) => return Err(GoalSkepticError::Failed(error.to_string())),
    }

    tokio::time::timeout(SKEPTIC_TURN_TIMEOUT, wait_for_skeptic_message(thread))
        .await
        .map_err(|_| GoalSkepticError::Failed("host skeptic turn timed out".into()))?
}

async fn wait_for_skeptic_message(
    thread: &codex_core::CodexThread,
) -> Result<crate::host_verify::GoalSkepticVote, GoalSkepticError> {
    loop {
        let event = thread
            .next_event()
            .await
            .map_err(|error| GoalSkepticError::Failed(error.to_string()))?;
        match event.msg {
            EventMsg::TurnComplete(complete) => {
                if let Some(error) = complete.error {
                    return Err(GoalSkepticError::Failed(format!(
                        "host skeptic turn failed: {}",
                        error.message
                    )));
                }
                let raw = complete.last_agent_message.ok_or_else(|| {
                    GoalSkepticError::Failed("host skeptic produced no final message".into())
                })?;
                return parse_goal_skeptic_vote(&raw).map_err(GoalSkepticError::from);
            }
            EventMsg::Error(error) => {
                return Err(GoalSkepticError::Failed(format!(
                    "host skeptic session error: {}",
                    error.message
                )));
            }
            EventMsg::TurnAborted(aborted) => {
                return Err(GoalSkepticError::Failed(format!(
                    "host skeptic turn aborted: {:?}",
                    aborted.reason
                )));
            }
            _ => {}
        }
    }
}

async fn shutdown_skeptic(thread_manager: &ThreadManager, spawned: NewThread) {
    let NewThread {
        thread_id, thread, ..
    } = spawned;
    if let Err(error) = tokio::time::timeout(SKEPTIC_SHUTDOWN_TIMEOUT, thread.shutdown_and_wait())
        .await
        .map_err(|_| {
            GoalSkepticError::Failed(format!("host skeptic {thread_id} shutdown timed out"))
        })
        .and_then(|result| result.map_err(|error| GoalSkepticError::Failed(error.to_string())))
    {
        tracing::warn!(error = %error, %thread_id, "failed to shut down host skeptic session");
    }
    thread_manager.remove_thread(&thread_id).await;
}

fn isolated_skeptic_config(parent_config: &Config) -> Result<Config, GoalSkepticError> {
    let mut config = parent_config.clone();
    config.base_instructions = Some(skeptic_system_prompt().to_string());
    config.base_instructions_provenance = Some(BaseInstructionsProvenance::Custom);
    config.developer_instructions = None;
    config.personality = None;
    config.include_apps_instructions = false;
    config.include_collaboration_mode_instructions = false;
    config.include_skill_instructions = false;
    config.orchestrator_skills_enabled = false;
    config.orchestrator_mcp_enabled = false;
    config.agents_enabled = false;
    config.memories.use_memories = false;
    config.memories.dedicated_tools = false;
    config.notify = None;
    config.token_budget = None;
    config.rollout_budget = None;
    config.max_goal_token_budget = None;
    config.project_doc_max_bytes = 0;
    config.ephemeral = true;
    config.permissions.approval_policy = Constrained::allow_only(AskForApproval::Never);
    config.mcp_servers = Constrained::allow_only(HashMap::new());
    let read_only = parent_config
        .permissions
        .permission_profile()
        .intersect_with_read_only()
        .unwrap_or(PermissionProfile::External {
            network: NetworkSandboxPolicy::Restricted,
        });
    if let Err(error) = config.permissions.set_permission_profile(read_only) {
        tracing::warn!(
            error = %error,
            "host skeptic could not set read-only permissions"
        );
    }
    for feature in [
        Feature::Apps,
        Feature::Collab,
        Feature::Goals,
        Feature::GoalHost,
        Feature::GuardianApproval,
        Feature::GuardianExt,
        Feature::GuardianV2,
        Feature::MemoryTool,
        Feature::MultiAgentV2,
        Feature::Plugins,
    ] {
        if let Err(error) = config.features.disable(feature) {
            tracing::warn!(
                error = %error,
                feature = feature.key(),
                "host skeptic could not disable feature"
            );
        }
    }
    Ok(config)
}
