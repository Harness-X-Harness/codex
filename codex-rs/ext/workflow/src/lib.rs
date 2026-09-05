//! Independent `/workflow` engine under the unified `goal_host` feature.
//!
//! This crate is the HOW layer. It does not complete goals and does not share
//! `/goal` tools, SQLite, or Grok's `.rhai` `WorkflowManager`.

mod extension;
mod run;
mod service;
mod spec;
mod steering;

pub use extension::WorkflowExtensionConfig;
pub use extension::install;
pub use run::WorkflowAdvance;
pub use run::WorkflowRun;
pub use run::WorkflowStatus;
pub use service::SharedWorkflowService;
pub use service::WorkflowService;
pub use service::WorkflowServiceError;
pub use spec::MAX_NAME_CHARS;
pub use spec::MAX_STEP_INSTRUCTION_CHARS;
pub use spec::MAX_WORKFLOW_SOURCE_CHARS;
pub use spec::MAX_WORKFLOW_STEPS;
pub use spec::WorkflowDefinition;
pub use spec::WorkflowParseError;
pub use spec::WorkflowStep;
pub use spec::parse_workflow_markdown;
