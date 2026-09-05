use super::App;
use crate::app_server_session::AppServerSession;
use crate::workflow_display::WORKFLOW_USAGE;
use crate::workflow_display::format_workflow_summary;
use codex_app_server_protocol::ThreadWorkflow;
use codex_protocol::ThreadId;

impl App {
    pub(super) async fn open_thread_workflow_status(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        let result = app_server.thread_workflow_get(thread_id).await;
        if self.current_displayed_thread_id() != Some(thread_id) {
            return;
        }

        let response = match result {
            Ok(response) => response,
            Err(err) => {
                self.chat_widget
                    .add_error_message(thread_workflow_error_message("read", &err));
                return;
            }
        };

        let Some(workflow) = response.workflow else {
            self.chat_widget.add_info_message(
                WORKFLOW_USAGE.to_string(),
                Some("No workflow is currently set.".to_string()),
            );
            return;
        };

        self.show_workflow_summary(workflow);
    }

    pub(super) async fn start_thread_workflow(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        source: String,
    ) {
        let result = app_server.thread_workflow_start(thread_id, source).await;
        self.show_workflow_mutation_result(thread_id, "start", result);
    }

    pub(super) async fn advance_thread_workflow(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        let result = app_server.thread_workflow_advance(thread_id).await;
        self.show_workflow_mutation_result(thread_id, "advance", result);
    }

    pub(super) async fn stop_thread_workflow(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        let result = app_server.thread_workflow_stop(thread_id).await;
        self.show_workflow_mutation_result(thread_id, "stop", result);
    }

    pub(super) async fn resume_thread_workflow(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        let result = app_server.thread_workflow_resume(thread_id).await;
        self.show_workflow_mutation_result(thread_id, "resume", result);
    }

    fn show_workflow_mutation_result(
        &mut self,
        thread_id: ThreadId,
        action: &str,
        result: color_eyre::Result<ThreadWorkflow>,
    ) {
        if self.current_displayed_thread_id() != Some(thread_id) {
            return;
        }
        match result {
            Ok(workflow) => self.show_workflow_summary(workflow),
            Err(err) => self
                .chat_widget
                .add_error_message(thread_workflow_error_message(action, &err)),
        }
    }

    fn show_workflow_summary(&mut self, workflow: ThreadWorkflow) {
        self.chat_widget
            .add_info_message(format_workflow_summary(&workflow), None);
    }
}

fn thread_workflow_error_message(action: &str, err: &color_eyre::Report) -> String {
    format!("Failed to {action} workflow: {err}")
}
