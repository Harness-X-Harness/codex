//! Independent `/workflow` Rhai HOW VM under the unified `goal_host` feature.
//!
//! This crate is the HOW layer. Completing a program ends the run only. It
//! does not write Goal state.

mod engine;
mod extension;
mod run;
mod service;
mod steering;

pub use engine::MAX_WORKFLOW_OPERATIONS;
pub use engine::MAX_WORKFLOW_REPLY_CHARS;
pub use engine::MAX_WORKFLOW_SOURCE_CHARS;
pub use engine::MAX_WORKFLOW_YIELDS;
pub use engine::WorkflowEval;
pub use engine::WorkflowSourceError;
pub use engine::eval_source;
pub use engine::truncate_workflow_reply;
pub use engine::validate_source;
pub use extension::WorkflowExtensionConfig;
pub use extension::install;
pub use run::WorkflowAdvance;
pub use run::WorkflowRun;
pub use run::WorkflowStatus;
pub use run::WorkflowStep;
pub use service::SharedWorkflowService;
pub use service::WorkflowService;
pub use service::WorkflowServiceError;
pub use service::WorkflowUpdateSink;
