use super::ChatWidget;
use crate::workflow_display::workflow_status_indicator_from_workflow;
use codex_app_server_protocol::ThreadWorkflow;
use codex_features::Feature;

impl ChatWidget {
    pub(super) fn on_thread_workflow_updated(&mut self, thread_id: &str, workflow: ThreadWorkflow) {
        if let Some(active_thread_id) = self.thread_id
            && active_thread_id.to_string() != thread_id
        {
            return;
        }
        if !self.config.features.enabled(Feature::GoalHost) {
            self.clear_workflow_status_indicator();
            return;
        }
        let indicator = workflow_status_indicator_from_workflow(&workflow);
        self.current_workflow_status_indicator = Some(indicator);
        self.bottom_pane
            .set_workflow_status_indicator(Some(indicator));
    }

    pub(super) fn clear_workflow_status_indicator(&mut self) {
        self.current_workflow_status_indicator = None;
        self.bottom_pane.set_workflow_status_indicator(None);
    }
}
