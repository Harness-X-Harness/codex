//! Production [`GoalRoundEvaluator`] that asks the thread's model for a JSON verdict.
//!
//! This is a tool-free, schema-constrained completion. It does not spawn worker
//! `spawn_agent` sessions. Host skeptics use Guardian internal sessions.

use std::sync::Arc;
use std::sync::Weak;

use codex_core::CodexResponsesMetadata;
use codex_core::ModelClient;
use codex_core::Prompt;
use codex_core::ResponseEvent;
use codex_core::ThreadConfigSnapshot;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_core::content_items_to_text;
use codex_core::resolve_installation_id;
use codex_features::Feature;
use codex_login::AgentIdentityAuthPolicy;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_otel::SessionTelemetry;
use codex_otel::TelemetryAuthMode;
use codex_protocol::ResponseItemId;
use codex_protocol::SessionId;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort;
use codex_rollout_trace::InferenceTraceContext;
use futures::StreamExt;

use crate::evaluator_prompt::EVALUATOR_SAMPLE_ATTEMPTS;
use crate::evaluator_prompt::build_goal_evaluator_user_payload;
use crate::evaluator_prompt::evaluator_system_prompt;
use crate::evaluator_prompt::goal_evaluator_evidence;
use crate::evaluator_prompt::goal_evaluator_output_schema;
use crate::host_evaluate::GoalEvaluatorError;
use crate::host_evaluate::GoalEvaluatorVerdict;
use crate::host_evaluate::GoalRoundEvaluationFuture;
use crate::host_evaluate::GoalRoundEvaluationInput;
use crate::host_evaluate::GoalRoundEvaluator;
use crate::host_evaluate::parse_goal_evaluator_verdict;

/// Host-owned round-end evaluator backed by the thread's current model.
pub struct ModelGoalRoundEvaluator {
    thread_manager: Weak<ThreadManager>,
    auth_manager: Arc<AuthManager>,
}

impl ModelGoalRoundEvaluator {
    pub fn new(thread_manager: Weak<ThreadManager>, auth_manager: Arc<AuthManager>) -> Self {
        Self {
            thread_manager,
            auth_manager,
        }
    }

    async fn evaluate_inner(
        &self,
        input: GoalRoundEvaluationInput,
    ) -> Result<GoalEvaluatorVerdict, GoalEvaluatorError> {
        let thread_manager = self.thread_manager.upgrade().ok_or_else(|| {
            GoalEvaluatorError::Failed("thread manager dropped before goal evaluation".into())
        })?;
        let thread = thread_manager
            .get_thread(input.thread_id)
            .await
            .map_err(|error| GoalEvaluatorError::Failed(error.to_string()))?;
        let history = thread
            .load_history(/*include_archived*/ false)
            .await
            .map_err(|error| GoalEvaluatorError::Failed(error.to_string()))?;
        let evidence = goal_evaluator_evidence(&history.items);
        let prompt = evaluator_prompt(
            &input.objective,
            &evidence.transcript,
            evidence.plan.as_deref(),
        );
        let config = thread.config().await;
        let snapshot = thread.config_snapshot().await;
        let model_name = snapshot.model.clone();
        if model_name.trim().is_empty() {
            return Err(GoalEvaluatorError::Failed(
                "goal evaluator is missing the thread model".into(),
            ));
        }
        let model_info = thread_manager
            .get_models_manager()
            .get_model_info(&model_name, &config.to_models_manager_config())
            .await;
        let reasoning_summary = snapshot
            .reasoning_summary
            .unwrap_or(model_info.default_reasoning_summary);
        let session_telemetry = session_telemetry(
            &self.auth_manager,
            &config,
            &snapshot,
            input.thread_id,
            &model_name,
        );
        let installation_id = resolve_installation_id(&config.codex_home)
            .await
            .map_err(|error| GoalEvaluatorError::Failed(error.to_string()))?;
        let responses_metadata = CodexResponsesMetadata::detached(
            installation_id,
            SessionId::from(input.thread_id).to_string(),
            input.thread_id.to_string(),
            format!("{}:goal-eval", input.thread_id),
        );

        let mut last_error =
            GoalEvaluatorError::Failed("goal evaluator produced no response".into());
        for _ in 0..EVALUATOR_SAMPLE_ATTEMPTS {
            match sample_evaluator_completion(
                &self.auth_manager,
                input.thread_id,
                &config,
                &snapshot,
                &prompt,
                &model_info,
                &session_telemetry,
                snapshot.reasoning_effort.clone(),
                reasoning_summary,
                snapshot.service_tier.clone(),
                &responses_metadata,
            )
            .await
            {
                Ok(raw) => match parse_goal_evaluator_verdict(&raw) {
                    Ok(verdict) => return Ok(verdict),
                    Err(error) => last_error = error.into(),
                },
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }
}

impl GoalRoundEvaluator for ModelGoalRoundEvaluator {
    fn evaluate(&self, input: GoalRoundEvaluationInput) -> GoalRoundEvaluationFuture<'_> {
        Box::pin(async move { self.evaluate_inner(input).await })
    }
}

fn evaluator_prompt(objective: &str, transcript: &str, plan: Option<&str>) -> Prompt {
    let mut prompt = Prompt::default();
    prompt.input = vec![ResponseItem::Message {
        id: Some(ResponseItemId::new("msg")),
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: build_goal_evaluator_user_payload(objective, transcript, plan),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }];
    prompt.base_instructions = BaseInstructions {
        text: evaluator_system_prompt().to_string(),
        provenance: None,
    };
    prompt.output_schema = Some(goal_evaluator_output_schema());
    prompt.output_schema_strict = true;
    prompt
}

fn session_telemetry(
    auth_manager: &AuthManager,
    config: &Config,
    snapshot: &ThreadConfigSnapshot,
    thread_id: ThreadId,
    model_name: &str,
) -> SessionTelemetry {
    let auth = auth_manager.auth_cached();
    let auth = auth.as_ref();
    SessionTelemetry::new(
        thread_id,
        model_name,
        model_name,
        auth.and_then(CodexAuth::get_account_id),
        auth.and_then(CodexAuth::get_account_email),
        auth.map(CodexAuth::auth_mode).map(TelemetryAuthMode::from),
        snapshot.originator.clone(),
        config.otel.log_user_prompt,
        "codex".to_string(),
        snapshot.session_source.clone(),
    )
}

#[allow(clippy::too_many_arguments)]
async fn sample_evaluator_completion(
    auth_manager: &Arc<AuthManager>,
    thread_id: ThreadId,
    config: &Config,
    snapshot: &ThreadConfigSnapshot,
    prompt: &Prompt,
    model_info: &ModelInfo,
    session_telemetry: &SessionTelemetry,
    effort: Option<ReasoningEffort>,
    summary: ReasoningSummary,
    service_tier: Option<String>,
    responses_metadata: &CodexResponsesMetadata,
) -> Result<String, GoalEvaluatorError> {
    let model_client = ModelClient::new(
        Some(Arc::clone(auth_manager)),
        AgentIdentityAuthPolicy::JwtOnly,
        thread_id,
        config.model_provider.clone(),
        snapshot.session_source.clone(),
        snapshot.originator.clone(),
        config.model_verbosity,
        config.features.enabled(Feature::ContentItemKinds),
        config.features.enabled(Feature::EnableRequestCompression),
        config.features.enabled(Feature::RuntimeMetrics),
        /*beta_features_header*/ None,
        /*concurrent_reasoning_summaries_enabled*/ false,
        /*attestation_provider*/ None,
        config.http_client_factory(),
    );
    let mut client_session = model_client.new_session();
    let mut stream = client_session
        .stream(
            prompt,
            model_info,
            session_telemetry,
            effort,
            summary,
            service_tier,
            responses_metadata,
            &InferenceTraceContext::disabled(),
        )
        .await
        .map_err(|error| GoalEvaluatorError::Failed(error.to_string()))?;

    let mut result = String::new();
    while let Some(message) = stream
        .next()
        .await
        .transpose()
        .map_err(|error| GoalEvaluatorError::Failed(error.to_string()))?
    {
        match message {
            ResponseEvent::OutputTextDelta(delta) => result.push_str(&delta),
            ResponseEvent::OutputItemDone(item) => {
                if result.is_empty()
                    && let ResponseItem::Message { content, .. } = item
                    && let Some(text) = content_items_to_text(&content)
                {
                    result.push_str(&text);
                }
            }
            ResponseEvent::Completed { .. } => break,
            _ => {}
        }
    }
    if result.trim().is_empty() {
        return Err(GoalEvaluatorError::Failed(
            "goal evaluator response contained no output text".into(),
        ));
    }
    Ok(result)
}
