//! Bounded Rhai HOW VM. Completing a program ends this run only.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use rhai::Dynamic;
use rhai::Engine;
use rhai::EvalAltResult;
use rhai::Position;
use rhai::Scope;

/// Inclusive cap on the source document.
pub const MAX_WORKFLOW_SOURCE_CHARS: usize = 32_000;
/// Inclusive cap on VM operations for one host resume.
pub const MAX_WORKFLOW_OPERATIONS: u64 = 50_000;
/// Inclusive cap on `ask` yields in one run.
pub const MAX_WORKFLOW_YIELDS: u32 = 32;
/// Inclusive cap on one `ask()` reply injected back into the VM.
pub const MAX_WORKFLOW_REPLY_CHARS: usize = 4_096;
/// Inclusive cap on Rhai call depth.
const MAX_CALL_LEVELS: usize = 32;

/// Why a workflow program could not be accepted or resumed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowSourceError {
    Empty,
    TooLarge { actual: usize },
    Invalid { reason: String },
}

impl fmt::Display for WorkflowSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("workflow source is empty"),
            Self::TooLarge { actual } => write!(
                f,
                "workflow source is {actual} characters; max is {MAX_WORKFLOW_SOURCE_CHARS}"
            ),
            Self::Invalid { reason } => f.write_str(reason),
        }
    }
}

impl std::error::Error for WorkflowSourceError {}

/// Result of one host-owned VM resume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowEval {
    Completed,
    Yielded { instruction: String },
}

#[derive(Clone, Debug)]
enum ControlToken {
    Complete,
    Yield(String),
}

const FORBIDDEN_GOAL_BINDINGS: &[&str] = &[
    "update_goal",
    "complete_goal",
    "block_goal",
    "set_goal",
    "mark_goal_complete",
    "mark_goal_blocked",
];

/// Compile-check a Rhai program without running it.
pub fn validate_source(source: &str) -> Result<(), WorkflowSourceError> {
    let source = source.trim();
    if source.is_empty() {
        return Err(WorkflowSourceError::Empty);
    }
    let actual = source.chars().count();
    if actual > MAX_WORKFLOW_SOURCE_CHARS {
        return Err(WorkflowSourceError::TooLarge { actual });
    }
    let engine = build_engine(&[]);
    engine
        .compile(source)
        .map(|_| ())
        .map_err(|error| WorkflowSourceError::Invalid {
            reason: format!("workflow program is not valid Rhai: {error}"),
        })
}

/// Run or resume a Rhai program. `served_replies` are host answers for
/// already-served `ask` calls, in program order.
pub fn eval_source(
    source: &str,
    served_replies: &[String],
) -> Result<WorkflowEval, WorkflowSourceError> {
    validate_source(source)?;
    if served_replies.len() > MAX_WORKFLOW_YIELDS as usize {
        return Err(WorkflowSourceError::Invalid {
            reason: format!("workflow exceeded {MAX_WORKFLOW_YIELDS} yields"),
        });
    }
    let engine = build_engine(served_replies);
    let ast = engine
        .compile(source)
        .map_err(|error| WorkflowSourceError::Invalid {
            reason: format!("workflow program is not valid Rhai: {error}"),
        })?;
    let mut scope = Scope::new();
    match engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast) {
        Ok(_) => Ok(WorkflowEval::Completed),
        Err(error) => outcome_from_error(*error),
    }
}

/// Bound a model reply before it re-enters the VM.
pub fn truncate_workflow_reply(reply: &str) -> String {
    let mut out = String::new();
    for ch in reply.chars() {
        if out.chars().count() >= MAX_WORKFLOW_REPLY_CHARS {
            break;
        }
        out.push(ch);
    }
    out
}

fn build_engine(served_replies: &[String]) -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(MAX_WORKFLOW_OPERATIONS);
    engine.set_max_call_levels(MAX_CALL_LEVELS);
    engine.set_max_string_size(MAX_WORKFLOW_SOURCE_CHARS);
    engine.disable_symbol("eval");
    engine.disable_symbol("import");

    engine.register_fn("complete", || -> Result<(), Box<EvalAltResult>> {
        Err(terminated(ControlToken::Complete))
    });
    engine.register_fn(
        "complete",
        |_value: Dynamic| -> Result<(), Box<EvalAltResult>> {
            Err(terminated(ControlToken::Complete))
        },
    );

    let replies = Rc::new(served_replies.to_vec());
    let index = Rc::new(Cell::new(0usize));
    let replies_for_ask = Rc::clone(&replies);
    let index_for_ask = Rc::clone(&index);
    engine.register_fn(
        "ask",
        move |instruction: &str| -> Result<String, Box<EvalAltResult>> {
            let i = index_for_ask.get();
            if i < replies_for_ask.len() {
                index_for_ask.set(i.saturating_add(1));
                return Ok(replies_for_ask[i].clone());
            }
            if instruction.trim().is_empty() {
                return Err(runtime_error("ask() requires a nonempty instruction"));
            }
            Err(terminated(ControlToken::Yield(instruction.to_string())))
        },
    );

    for name in FORBIDDEN_GOAL_BINDINGS {
        let binding = (*name).to_string();
        engine.register_fn(binding.as_str(), || -> Result<(), Box<EvalAltResult>> {
            Err(runtime_error(GOAL_BINDING_ERROR))
        });
        engine.register_fn(
            binding.as_str(),
            |_value: Dynamic| -> Result<(), Box<EvalAltResult>> {
                Err(runtime_error(GOAL_BINDING_ERROR))
            },
        );
    }

    engine
}

const GOAL_BINDING_ERROR: &str = "host bindings cannot commit goal complete or blocked";

fn outcome_from_error(error: EvalAltResult) -> Result<WorkflowEval, WorkflowSourceError> {
    if let Some(token) = find_control_token(&error) {
        return match token {
            ControlToken::Complete => Ok(WorkflowEval::Completed),
            ControlToken::Yield(instruction) => Ok(WorkflowEval::Yielded { instruction }),
        };
    }
    Err(WorkflowSourceError::Invalid {
        reason: error.to_string(),
    })
}

fn find_control_token(error: &EvalAltResult) -> Option<ControlToken> {
    match error {
        EvalAltResult::ErrorTerminated(token, _) => token.clone().try_cast::<ControlToken>(),
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => find_control_token(inner),
        EvalAltResult::ErrorInModule(_, inner, _) => find_control_token(inner),
        _ => None,
    }
}

fn terminated(token: ControlToken) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorTerminated(
        Dynamic::from(token),
        Position::NONE,
    ))
}

fn runtime_error(message: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        Dynamic::from(message.into()),
        Position::NONE,
    ))
}
