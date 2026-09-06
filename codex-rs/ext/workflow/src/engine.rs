//! Bounded Rhai HOW VM. Completing a program ends this run only.

use std::cell::Cell;
use std::cell::RefCell;
use std::fmt;
use std::path::Path;
use std::rc::Rc;

use rhai::Array;
use rhai::Dynamic;
use rhai::Engine;
use rhai::EvalAltResult;
use rhai::Map;
use rhai::Position;
use rhai::Scope;
use sha2::Digest;
use sha2::Sha256;

use crate::scratch;

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
    Paused,
}

#[derive(Clone, Debug)]
enum ControlToken {
    Complete(serde_json::Value),
    Yield(String),
    Pause,
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
    let engine = build_engine(&[], 0, ScratchBinding::Unavailable);
    engine
        .compile(source)
        .map(|_| ())
        .map_err(|error| WorkflowSourceError::Invalid {
            reason: format!("workflow program is not valid Rhai: {error}"),
        })
}

/// Run or resume a Rhai program. `served_replies` are host answers for
/// already-served `ask` and `agent` yields, in program order.
pub fn eval_source(
    source: &str,
    served_replies: &[String],
) -> Result<WorkflowEval, WorkflowSourceError> {
    eval_source_with_pauses(source, served_replies, 0)
}

/// Resume a program with replayed host replies and consumed `pause` / `await_user` calls.
pub fn eval_source_with_pauses(
    source: &str,
    served_replies: &[String],
    served_pauses: u32,
) -> Result<WorkflowEval, WorkflowSourceError> {
    eval_source_with_env(source, served_replies, served_pauses, &Map::new())
        .map(|outcome| outcome.eval)
}

/// Resume a program with replayed host replies, consumed pauses, and `args`.
pub fn eval_source_with_env(
    source: &str,
    served_replies: &[String],
    served_pauses: u32,
    args: &Map,
) -> Result<WorkflowEvalOutcome, WorkflowSourceError> {
    eval_source_inner(
        source,
        served_replies,
        served_pauses,
        args,
        ScratchBinding::Unavailable,
    )
}

/// Resume a program with a thread-local scratch directory.
pub fn eval_source_with_scratch(
    source: &str,
    served_replies: &[String],
    served_pauses: u32,
    args: &Map,
    scratch_dir: &Path,
) -> Result<WorkflowEvalOutcome, WorkflowSourceError> {
    eval_source_inner(
        source,
        served_replies,
        served_pauses,
        args,
        ScratchBinding::Directory(scratch_dir),
    )
}

enum ScratchBinding<'a> {
    Unavailable,
    Directory(&'a Path),
}

fn eval_source_inner(
    source: &str,
    served_replies: &[String],
    served_pauses: u32,
    args: &Map,
    scratch: ScratchBinding<'_>,
) -> Result<WorkflowEvalOutcome, WorkflowSourceError> {
    validate_source(source)?;
    if served_replies.len() > MAX_WORKFLOW_YIELDS as usize {
        return Err(WorkflowSourceError::Invalid {
            reason: format!("workflow exceeded {MAX_WORKFLOW_YIELDS} yields"),
        });
    }
    if served_pauses > MAX_WORKFLOW_YIELDS {
        return Err(WorkflowSourceError::Invalid {
            reason: format!("workflow exceeded {MAX_WORKFLOW_YIELDS} pauses"),
        });
    }
    let phase = Rc::new(RefCell::new(None));
    let log = Rc::new(RefCell::new(None));
    let mut engine = build_engine(served_replies, served_pauses, scratch);
    let phase_for_fn = Rc::clone(&phase);
    engine.register_fn(
        "phase",
        move |title: &str| -> Result<(), Box<EvalAltResult>> {
            if title.trim().is_empty() {
                return Err(runtime_error("phase() requires a nonempty title"));
            }
            *phase_for_fn.borrow_mut() = Some(title.to_string());
            Ok(())
        },
    );
    let log_for_fn = Rc::clone(&log);
    engine.register_fn(
        "log",
        move |message: &str| -> Result<(), Box<EvalAltResult>> {
            if message.trim().is_empty() {
                return Err(runtime_error("log() requires a nonempty message"));
            }
            *log_for_fn.borrow_mut() = Some(message.to_string());
            Ok(())
        },
    );
    let ast = engine
        .compile(source)
        .map_err(|error| WorkflowSourceError::Invalid {
            reason: format!("workflow program is not valid Rhai: {error}"),
        })?;
    let mut scope = Scope::new();
    scope.push_dynamic("args", Dynamic::from(args.clone()));
    let (eval, result) = match engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast) {
        Ok(_) => (WorkflowEval::Completed, serde_json::Value::Null),
        Err(error) => outcome_from_error(*error)?,
    };
    Ok(WorkflowEvalOutcome {
        eval,
        phase: phase.borrow().clone(),
        log: log.borrow().clone(),
        result,
    })
}

/// Result of one VM resume, including the last `phase` title, `log` message, and this-run result.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowEvalOutcome {
    pub eval: WorkflowEval,
    pub phase: Option<String>,
    pub log: Option<String>,
    pub result: serde_json::Value,
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

fn build_engine(
    served_replies: &[String],
    served_pauses: u32,
    scratch: ScratchBinding<'_>,
) -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(MAX_WORKFLOW_OPERATIONS);
    engine.set_max_call_levels(MAX_CALL_LEVELS);
    engine.set_max_string_size(MAX_WORKFLOW_SOURCE_CHARS);
    engine.disable_symbol("eval");
    engine.disable_symbol("import");

    engine.register_fn("complete", || -> Result<(), Box<EvalAltResult>> {
        Err(terminated(ControlToken::Complete(serde_json::Value::Null)))
    });
    engine.register_fn(
        "complete",
        |value: Dynamic| -> Result<(), Box<EvalAltResult>> {
            let json = dynamic_to_json(value)?;
            let encoded = serde_json::to_string(&json)
                .map_err(|error| runtime_error(format!("complete() failed: {error}")))?;
            if encoded.chars().count() > MAX_WORKFLOW_SOURCE_CHARS {
                return Err(runtime_error(format!(
                    "complete() exceeds {MAX_WORKFLOW_SOURCE_CHARS} characters"
                )));
            }
            Err(terminated(ControlToken::Complete(json)))
        },
    );
    engine.register_fn("fingerprint", |text: &str| -> String {
        format!("{:x}", Sha256::digest(text.as_bytes()))
    });
    engine.register_fn(
        "json_encode",
        |value: Dynamic| -> Result<String, Box<EvalAltResult>> {
            let encoded = serde_json::to_string(&dynamic_to_json(value)?)
                .map_err(|error| runtime_error(format!("json_encode() failed: {error}")))?;
            if encoded.chars().count() > MAX_WORKFLOW_SOURCE_CHARS {
                return Err(runtime_error(format!(
                    "json_encode() exceeds {MAX_WORKFLOW_SOURCE_CHARS} characters"
                )));
            }
            Ok(encoded)
        },
    );

    let replies = Rc::new(served_replies.to_vec());
    let index = Rc::new(Cell::new(0usize));
    let replies_for_ask = Rc::clone(&replies);
    let index_for_ask = Rc::clone(&index);
    engine.register_fn(
        "ask",
        move |instruction: &str| -> Result<String, Box<EvalAltResult>> {
            take_served_or_yield(
                &index_for_ask,
                &replies_for_ask,
                instruction,
                "ask() requires a nonempty instruction",
            )
        },
    );

    let replies_for_agent = Rc::clone(&replies);
    let index_for_agent = Rc::clone(&index);
    engine.register_fn(
        "agent",
        move |prompt: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            take_served_or_yield(
                &index_for_agent,
                &replies_for_agent,
                prompt,
                "agent() requires a nonempty prompt",
            )
            .map(|reply| agent_result_from_reply(&reply))
        },
    );
    let replies_for_agent_opts = Rc::clone(&replies);
    let index_for_agent_opts = Rc::clone(&index);
    engine.register_fn(
        "agent",
        move |prompt: &str, _opts: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            take_served_or_yield(
                &index_for_agent_opts,
                &replies_for_agent_opts,
                prompt,
                "agent() requires a nonempty prompt",
            )
            .map(|reply| agent_result_from_reply(&reply))
        },
    );

    let index_for_budget = Rc::clone(&index);
    engine.register_fn("budget", move || -> Result<Dynamic, Box<EvalAltResult>> {
        let spent = i64::try_from(index_for_budget.get()).unwrap_or(i64::MAX);
        let total = i64::from(MAX_WORKFLOW_YIELDS);
        let remaining = total.saturating_sub(spent);
        let mut map = Map::new();
        map.insert("total".into(), Dynamic::from(total));
        map.insert("spent".into(), Dynamic::from(spent));
        map.insert("reserved".into(), Dynamic::from(0_i64));
        map.insert("remaining".into(), Dynamic::from(remaining));
        Ok(Dynamic::from(map))
    });

    let replies_for_parallel = Rc::clone(&replies);
    let index_for_parallel = Rc::clone(&index);
    engine.register_fn(
        "parallel",
        move |items: Array| -> Result<Array, Box<EvalAltResult>> {
            let remaining = (MAX_WORKFLOW_YIELDS as usize).saturating_sub(index_for_parallel.get());
            if items.len() > remaining {
                return Err(runtime_error(format!(
                    "parallel() exceeds the remaining yield budget (need {}, have {remaining})",
                    items.len()
                )));
            }
            let mut results = Array::with_capacity(items.len());
            for item in items {
                let map = item
                    .try_cast::<Map>()
                    .ok_or_else(|| runtime_error("parallel() items must be option maps"))?;
                let prompt = match map.get("prompt") {
                    Some(value) => value
                        .clone()
                        .into_string()
                        .map_err(|_| runtime_error("parallel() requires a nonempty prompt"))?,
                    None => {
                        return Err(runtime_error("parallel() requires a nonempty prompt"));
                    }
                };
                let reply = take_served_or_yield(
                    &index_for_parallel,
                    &replies_for_parallel,
                    &prompt,
                    "parallel() requires a nonempty prompt",
                )?;
                results.push(agent_result_from_reply(&reply));
            }
            Ok(results)
        },
    );

    let scratch_dir = match scratch {
        ScratchBinding::Unavailable => None,
        ScratchBinding::Directory(dir) => Some(dir.to_path_buf()),
    };
    let scratch_for_write = scratch_dir.clone();
    engine.register_fn(
        "write_scratch_file",
        move |name: &str, content: &str| -> Result<String, Box<EvalAltResult>> {
            let Some(dir) = scratch_for_write.as_ref() else {
                return Err(runtime_error("scratch is unavailable"));
            };
            scratch::write_scratch_file(dir, name, content).map_err(runtime_error)
        },
    );
    let scratch_for_read = scratch_dir;
    engine.register_fn(
        "read_scratch_file",
        move |name: &str| -> Result<String, Box<EvalAltResult>> {
            let Some(dir) = scratch_for_read.as_ref() else {
                return Err(runtime_error("scratch is unavailable"));
            };
            scratch::read_scratch_file(dir, name).map_err(runtime_error)
        },
    );

    let pause_index = Rc::new(Cell::new(0u32));
    let pause_for_pause = Rc::clone(&pause_index);
    engine.register_fn("pause", move || -> Result<(), Box<EvalAltResult>> {
        take_served_or_pause(&pause_for_pause, served_pauses)
    });
    let pause_for_await = Rc::clone(&pause_index);
    engine.register_fn("await_user", move || -> Result<(), Box<EvalAltResult>> {
        take_served_or_pause(&pause_for_await, served_pauses)
    });

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

fn outcome_from_error(
    error: EvalAltResult,
) -> Result<(WorkflowEval, serde_json::Value), WorkflowSourceError> {
    if let Some(token) = find_control_token(&error) {
        return match token {
            ControlToken::Complete(value) => Ok((WorkflowEval::Completed, value)),
            ControlToken::Yield(instruction) => Ok((
                WorkflowEval::Yielded { instruction },
                serde_json::Value::Null,
            )),
            ControlToken::Pause => Ok((WorkflowEval::Paused, serde_json::Value::Null)),
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

fn dynamic_to_json(value: Dynamic) -> Result<serde_json::Value, Box<EvalAltResult>> {
    if value.is_unit() {
        return Ok(serde_json::Value::Null);
    }
    if value.is_bool() {
        let flag = value
            .as_bool()
            .map_err(|_| runtime_error("JSON conversion requires a bool"))?;
        return Ok(serde_json::Value::Bool(flag));
    }
    if value.is_int() {
        let number = value
            .as_int()
            .map_err(|_| runtime_error("JSON conversion requires an integer"))?;
        return Ok(serde_json::Value::Number(number.into()));
    }
    if value.is_float() {
        let number = value
            .as_float()
            .map_err(|_| runtime_error("JSON conversion requires a float"))?;
        let encoded = serde_json::Number::from_f64(number)
            .ok_or_else(|| runtime_error("JSON conversion requires a finite float"))?;
        return Ok(serde_json::Value::Number(encoded));
    }
    if value.is_string() {
        let text = value
            .into_string()
            .map_err(|_| runtime_error("JSON conversion requires a string"))?;
        return Ok(serde_json::Value::String(text));
    }
    if value.is_array() {
        let items = value
            .into_array()
            .map_err(|_| runtime_error("JSON conversion requires an array"))?;
        let mut encoded = Vec::with_capacity(items.len());
        for item in items {
            encoded.push(dynamic_to_json(item)?);
        }
        return Ok(serde_json::Value::Array(encoded));
    }
    if value.is_map() {
        let map = value
            .try_cast::<Map>()
            .ok_or_else(|| runtime_error("JSON conversion requires a map"))?;
        let mut object = serde_json::Map::new();
        for (key, item) in map {
            object.insert(key.to_string(), dynamic_to_json(item)?);
        }
        return Ok(serde_json::Value::Object(object));
    }
    Err(runtime_error(format!(
        "JSON conversion does not accept {}",
        value.type_name()
    )))
}

fn take_served_or_yield(
    index: &Rc<Cell<usize>>,
    replies: &Rc<Vec<String>>,
    instruction: &str,
    empty_error: &str,
) -> Result<String, Box<EvalAltResult>> {
    let i = index.get();
    if i < replies.len() {
        index.set(i.saturating_add(1));
        return Ok(replies[i].clone());
    }
    if instruction.trim().is_empty() {
        return Err(runtime_error(empty_error));
    }
    Err(terminated(ControlToken::Yield(instruction.to_string())))
}

fn take_served_or_pause(
    index: &Rc<Cell<u32>>,
    served_pauses: u32,
) -> Result<(), Box<EvalAltResult>> {
    let i = index.get();
    if i < served_pauses {
        index.set(i.saturating_add(1));
        return Ok(());
    }
    Err(terminated(ControlToken::Pause))
}

fn agent_result_from_reply(reply: &str) -> Dynamic {
    let mut map = Map::new();
    map.insert("ok".into(), Dynamic::from(!reply.trim().is_empty()));
    map.insert("text".into(), Dynamic::from(reply.to_string()));
    Dynamic::from(map)
}
