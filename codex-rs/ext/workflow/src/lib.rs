//! Independent `/workflow` Rhai HOW VM under the unified `goal_host` feature.
//!
//! This crate is the HOW layer. Completing a program ends the run only. It
//! does not write Goal state.

mod catalog;
mod engine;
mod extension;
mod run;
mod scratch;
mod service;
mod steering;

pub use catalog::CatalogError;
pub use catalog::CatalogRoots;
pub use catalog::CatalogScript;
pub use catalog::looks_like_catalog_name;
pub use catalog::normalize_catalog_name;
pub use catalog::resolve_named;
pub use engine::MAX_WORKFLOW_OPERATIONS;
pub use engine::MAX_WORKFLOW_REPLY_CHARS;
pub use engine::MAX_WORKFLOW_SOURCE_CHARS;
pub use engine::MAX_WORKFLOW_YIELDS;
pub use engine::SpawnBinding;
pub use engine::WorkflowEval;
pub use engine::WorkflowEvalOutcome;
pub use engine::WorkflowSourceError;
pub use engine::eval_source;
pub use engine::eval_source_with_env;
pub use engine::eval_source_with_pauses;
pub use engine::eval_source_with_scratch;
pub use engine::eval_source_with_scratch_and_spawn;
pub use engine::eval_source_with_spawn;
pub use engine::truncate_workflow_reply;
pub use engine::validate_source;
pub use extension::WorkflowExtensionConfig;
pub use extension::install;
pub use run::WorkflowAdvance;
pub use run::WorkflowRun;
pub use run::WorkflowStatus;
pub use service::SharedWorkflowService;
pub use service::WorkflowService;
pub use service::WorkflowServiceError;
pub use service::WorkflowUpdateSink;
