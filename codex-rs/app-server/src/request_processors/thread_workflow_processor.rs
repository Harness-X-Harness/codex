use std::sync::Arc;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::outgoing_message::OutgoingMessageSender;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadWorkflow;
use codex_app_server_protocol::ThreadWorkflowAdvanceParams;
use codex_app_server_protocol::ThreadWorkflowAdvanceResponse;
use codex_app_server_protocol::ThreadWorkflowGetParams;
use codex_app_server_protocol::ThreadWorkflowGetResponse;
use codex_app_server_protocol::ThreadWorkflowResumeParams;
use codex_app_server_protocol::ThreadWorkflowResumeResponse;
use codex_app_server_protocol::ThreadWorkflowStartParams;
use codex_app_server_protocol::ThreadWorkflowStartResponse;
use codex_app_server_protocol::ThreadWorkflowStatus;
use codex_app_server_protocol::ThreadWorkflowStep;
use codex_app_server_protocol::ThreadWorkflowStopParams;
use codex_app_server_protocol::ThreadWorkflowStopResponse;
use codex_app_server_protocol::ThreadWorkflowUpdatedNotification;
use codex_core::config::Config;
use codex_features::Feature;
use codex_protocol::ThreadId;
use codex_workflow_extension::WorkflowRun;
use codex_workflow_extension::WorkflowService;
use codex_workflow_extension::WorkflowServiceError;
use codex_workflow_extension::WorkflowStatus;

pub(crate) struct ThreadWorkflowRequestProcessor {
    outgoing: Arc<OutgoingMessageSender>,
    config: Arc<Config>,
    service: Arc<WorkflowService>,
}

impl ThreadWorkflowRequestProcessor {
    pub(crate) fn new(
        outgoing: Arc<OutgoingMessageSender>,
        config: Arc<Config>,
        service: Arc<WorkflowService>,
    ) -> Self {
        Self {
            outgoing,
            config,
            service,
        }
    }

    pub(crate) async fn get(
        &self,
        params: ThreadWorkflowGetParams,
    ) -> Result<ThreadWorkflowGetResponse, JSONRPCErrorError> {
        self.require_goal_host()?;
        let thread_id = parse_thread_id(&params.thread_id)?;
        let workflow = self
            .service
            .get_run(thread_id)
            .await
            .map_err(workflow_service_error)?
            .map(api_workflow);
        Ok(ThreadWorkflowGetResponse { workflow })
    }

    pub(crate) async fn start(
        &self,
        params: ThreadWorkflowStartParams,
    ) -> Result<ThreadWorkflowStartResponse, JSONRPCErrorError> {
        self.require_goal_host()?;
        let thread_id = parse_thread_id(&params.thread_id)?;
        let run = self
            .service
            .start_run(thread_id, &params.source)
            .await
            .map_err(workflow_service_error)?;
        Ok(ThreadWorkflowStartResponse {
            workflow: self.emit_updated(run).await,
        })
    }

    pub(crate) async fn advance(
        &self,
        params: ThreadWorkflowAdvanceParams,
    ) -> Result<ThreadWorkflowAdvanceResponse, JSONRPCErrorError> {
        self.require_goal_host()?;
        let thread_id = parse_thread_id(&params.thread_id)?;
        let run = self
            .service
            .advance_run(thread_id)
            .await
            .map_err(workflow_service_error)?;
        Ok(ThreadWorkflowAdvanceResponse {
            workflow: self.emit_updated(run).await,
        })
    }

    pub(crate) async fn stop(
        &self,
        params: ThreadWorkflowStopParams,
    ) -> Result<ThreadWorkflowStopResponse, JSONRPCErrorError> {
        self.require_goal_host()?;
        let thread_id = parse_thread_id(&params.thread_id)?;
        let run = self
            .service
            .stop_run(thread_id)
            .await
            .map_err(workflow_service_error)?;
        Ok(ThreadWorkflowStopResponse {
            workflow: self.emit_updated(run).await,
        })
    }

    pub(crate) async fn resume(
        &self,
        params: ThreadWorkflowResumeParams,
    ) -> Result<ThreadWorkflowResumeResponse, JSONRPCErrorError> {
        self.require_goal_host()?;
        let thread_id = parse_thread_id(&params.thread_id)?;
        let run = self
            .service
            .resume_run(thread_id)
            .await
            .map_err(workflow_service_error)?;
        Ok(ThreadWorkflowResumeResponse {
            workflow: self.emit_updated(run).await,
        })
    }

    fn require_goal_host(&self) -> Result<(), JSONRPCErrorError> {
        if self.config.features.enabled(Feature::GoalHost) {
            Ok(())
        } else {
            Err(invalid_request("goal_host feature is disabled"))
        }
    }

    async fn emit_updated(&self, run: WorkflowRun) -> ThreadWorkflow {
        let workflow = api_workflow(run);
        self.outgoing
            .send_server_notification(ServerNotification::ThreadWorkflowUpdated(
                ThreadWorkflowUpdatedNotification {
                    thread_id: workflow.thread_id.clone(),
                    workflow: workflow.clone(),
                },
            ))
            .await;
        workflow
    }
}

fn api_workflow(run: WorkflowRun) -> ThreadWorkflow {
    ThreadWorkflow {
        thread_id: run.thread_id.to_string(),
        run_id: run.run_id,
        name: run.name,
        status: match run.status {
            WorkflowStatus::Active => ThreadWorkflowStatus::Active,
            WorkflowStatus::Paused => ThreadWorkflowStatus::Paused,
            WorkflowStatus::Complete => ThreadWorkflowStatus::Complete,
        },
        current_step_index: u32::try_from(run.current_step_index).unwrap_or(u32::MAX),
        steps: run
            .steps
            .into_iter()
            .map(|step| ThreadWorkflowStep {
                id: step.id,
                title: step.title,
                instruction: step.instruction,
            })
            .collect(),
        created_at: run.created_at,
        updated_at: run.updated_at,
    }
}

fn workflow_service_error(err: WorkflowServiceError) -> JSONRPCErrorError {
    match err {
        WorkflowServiceError::InvalidRequest(message) => invalid_request(message),
        WorkflowServiceError::Internal(message) => internal_error(message),
    }
}

fn parse_thread_id(thread_id: &str) -> Result<ThreadId, JSONRPCErrorError> {
    ThreadId::from_string(thread_id)
        .map_err(|err| invalid_request(format!("invalid thread id: {err}")))
}
