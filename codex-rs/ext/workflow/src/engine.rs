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

use crate::host_opts;
use crate::journal::ContinuationKind;
use crate::journal::ContinuationRecord;
use crate::journal::HostCallResult;
use crate::journal::JournalLookup;
use crate::journal::REPLAY_DIVERGENCE;
use crate::journal::agent_request;
use crate::journal::ask_request;
use crate::journal::bounded_counts;
use crate::journal::control_request;
use crate::journal::lookup;
use crate::journal::request_digest;
use crate::journal::spawn_request;
use crate::scratch;

/// Inclusive cap on the source document.
pub const MAX_WORKFLOW_SOURCE_CHARS: usize = 32_000;
/// Inclusive UTF-8 byte cap: four bytes per source character plus a
/// truncated-codepoint allowance. Used before allocating a file read.
pub const MAX_WORKFLOW_SOURCE_BYTES: usize = MAX_WORKFLOW_SOURCE_CHARS * 4 + 4;
/// Inclusive cap on VM operations for one host resume.
pub const MAX_WORKFLOW_OPERATIONS: u64 = 50_000;
/// Inclusive cap on result-bearing host yields (`ask` / `agent` /
/// `spawn_agent`) in one run.
pub const MAX_WORKFLOW_YIELDS: u32 = 32;
/// Inclusive cap on `pause` / `await_user` control resumes in one run.
pub const MAX_WORKFLOW_CONTROL_RESUMES: u32 = 32;
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
    Yield {
        instruction: String,
        kind: ContinuationKind,
        request_digest: String,
    },
    Pause {
        kind: ContinuationKind,
        request_digest: String,
    },
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
    let engine = build_engine(
        &[],
        ScratchBinding::Unavailable,
        SpawnBinding::Unavailable,
        Rc::new(RefCell::new(None)),
    );
    engine
        .compile(source)
        .map(|_| ())
        .map_err(|error| WorkflowSourceError::Invalid {
            reason: format!("workflow program is not valid Rhai: {error}"),
        })
}

/// Run or resume a Rhai program. `journal` is the identity-checked
/// continuation log; positional replies alone are not replay identity.
pub fn eval_source(
    source: &str,
    journal: &[ContinuationRecord],
) -> Result<WorkflowEval, WorkflowSourceError> {
    eval_source_with_env(source, journal, &Map::new()).map(|outcome| outcome.eval)
}

/// Resume a program with an identity-checked journal and `args`.
pub fn eval_source_with_env(
    source: &str,
    journal: &[ContinuationRecord],
    args: &Map,
) -> Result<WorkflowEvalOutcome, WorkflowSourceError> {
    eval_source_inner(
        source,
        journal,
        args,
        ScratchBinding::Unavailable,
        SpawnBinding::Unavailable,
    )
}

/// Resume a program with a thread-local scratch directory.
pub fn eval_source_with_scratch(
    source: &str,
    journal: &[ContinuationRecord],
    args: &Map,
    scratch_dir: &Path,
) -> Result<WorkflowEvalOutcome, WorkflowSourceError> {
    eval_source_inner(
        source,
        journal,
        args,
        ScratchBinding::Directory(scratch_dir),
        SpawnBinding::Unavailable,
    )
}

enum ScratchBinding<'a> {
    Unavailable,
    Directory(&'a Path),
}

/// Whether stock Multi-Agent V2 `spawn_agent` can be requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpawnBinding {
    Unavailable,
    Available,
}

/// Resume a program with an explicit spawn-agent binding.
pub fn eval_source_with_spawn(
    source: &str,
    journal: &[ContinuationRecord],
    args: &Map,
    spawn: SpawnBinding,
) -> Result<WorkflowEvalOutcome, WorkflowSourceError> {
    eval_source_inner(source, journal, args, ScratchBinding::Unavailable, spawn)
}

/// Resume a program with scratch files and an explicit spawn-agent binding.
pub fn eval_source_with_scratch_and_spawn(
    source: &str,
    journal: &[ContinuationRecord],
    args: &Map,
    scratch_dir: &Path,
    spawn: SpawnBinding,
) -> Result<WorkflowEvalOutcome, WorkflowSourceError> {
    eval_source_inner(
        source,
        journal,
        args,
        ScratchBinding::Directory(scratch_dir),
        spawn,
    )
}

fn eval_source_inner(
    source: &str,
    journal: &[ContinuationRecord],
    args: &Map,
    scratch: ScratchBinding<'_>,
    spawn: SpawnBinding,
) -> Result<WorkflowEvalOutcome, WorkflowSourceError> {
    validate_source(source)?;
    if let Err(reason) = bounded_counts(journal) {
        return Err(WorkflowSourceError::Invalid { reason });
    }
    let phase = Rc::new(RefCell::new(None));
    let log = Rc::new(RefCell::new(None));
    let spawn_task_name = Rc::new(RefCell::new(None));
    let mut engine = build_engine(journal, scratch, spawn, Rc::clone(&spawn_task_name));
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
    let (eval, result, yield_kind, yield_request_digest) =
        match engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast) {
            Ok(_) => (WorkflowEval::Completed, serde_json::Value::Null, None, None),
            Err(error) => outcome_from_error(*error)?,
        };
    Ok(WorkflowEvalOutcome {
        eval,
        phase: phase.borrow().clone(),
        log: log.borrow().clone(),
        result,
        spawn_task_name: spawn_task_name.borrow().clone(),
        yield_kind,
        yield_request_digest,
    })
}

/// Result of one VM resume, including the last `phase` title, `log` message, this-run result, and spawn request.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowEvalOutcome {
    pub eval: WorkflowEval,
    pub phase: Option<String>,
    pub log: Option<String>,
    pub result: serde_json::Value,
    pub spawn_task_name: Option<String>,
    pub yield_kind: Option<ContinuationKind>,
    pub yield_request_digest: Option<String>,
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
    journal: &[ContinuationRecord],
    scratch: ScratchBinding<'_>,
    spawn: SpawnBinding,
    pending_spawn: Rc<RefCell<Option<String>>>,
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

    let journal = Rc::new(journal.to_vec());
    let index = Rc::new(Cell::new(0usize));
    let yield_index = Rc::new(Cell::new(0usize));
    let control_index = Rc::new(Cell::new(0usize));
    let journal_for_ask = Rc::clone(&journal);
    let index_for_ask = Rc::clone(&index);
    let yield_index_for_ask = Rc::clone(&yield_index);
    engine.register_fn(
        "ask",
        move |instruction: &str| -> Result<String, Box<EvalAltResult>> {
            take_journal_or_yield(
                &index_for_ask,
                &yield_index_for_ask,
                &journal_for_ask,
                ContinuationKind::Ask,
                &ask_request(instruction),
                instruction,
                "ask() requires a nonempty instruction",
            )
            .map(|result| result.text)
        },
    );

    let journal_for_agent = Rc::clone(&journal);
    let index_for_agent = Rc::clone(&index);
    let yield_index_for_agent = Rc::clone(&yield_index);
    let pending_for_agent = Rc::clone(&pending_spawn);
    engine.register_fn(
        "agent",
        move |prompt: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            take_agent_call(
                prompt,
                /*spawn_task_name*/ None,
                spawn,
                &index_for_agent,
                &yield_index_for_agent,
                &journal_for_agent,
                &pending_for_agent,
            )
        },
    );
    let journal_for_agent_opts = Rc::clone(&journal);
    let index_for_agent_opts = Rc::clone(&index);
    let yield_index_for_agent_opts = Rc::clone(&yield_index);
    let pending_for_agent_opts = Rc::clone(&pending_spawn);
    engine.register_fn(
        "agent",
        move |prompt: &str, opts: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            take_agent_call(
                prompt,
                host_opts::spawn_task_name(&opts)?,
                spawn,
                &index_for_agent_opts,
                &yield_index_for_agent_opts,
                &journal_for_agent_opts,
                &pending_for_agent_opts,
            )
        },
    );

    let index_for_budget = Rc::clone(&yield_index);
    engine.register_fn(
        "yield_budget",
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            let spent = i64::try_from(index_for_budget.get()).unwrap_or(i64::MAX);
            let total = i64::from(MAX_WORKFLOW_YIELDS);
            let remaining = total.saturating_sub(spent);
            let mut map = Map::new();
            map.insert("total".into(), Dynamic::from(total));
            map.insert("spent".into(), Dynamic::from(spent));
            map.insert("remaining".into(), Dynamic::from(remaining));
            Ok(Dynamic::from(map))
        },
    );
    engine.register_fn("budget", || -> Result<Dynamic, Box<EvalAltResult>> {
        Err(runtime_error(
            "budget() was renamed to yield_budget(); it reports the workflow yield allowance, not a token ledger",
        ))
    });

    let journal_for_batch = Rc::clone(&journal);
    let index_for_batch = Rc::clone(&index);
    let yield_index_for_batch = Rc::clone(&yield_index);
    let pending_for_batch = Rc::clone(&pending_spawn);
    engine.register_fn(
        "batch_agent",
        move |items: Array| -> Result<Array, Box<EvalAltResult>> {
            let remaining =
                (MAX_WORKFLOW_YIELDS as usize).saturating_sub(yield_index_for_batch.get());
            if items.len() > remaining {
                return Err(runtime_error(format!(
                    "batch_agent() exceeds the remaining yield budget (need {}, have {remaining})",
                    items.len()
                )));
            }
            let mut prepared = Vec::with_capacity(items.len());
            for item in items {
                let map = item
                    .try_cast::<Map>()
                    .ok_or_else(|| runtime_error("batch_agent() items must be option maps"))?;
                let (prompt, spawn_task_name) = host_opts::batch_item_prompt_and_spawn(&map)?;
                prepared.push((prompt, spawn_task_name));
            }
            let mut results = Array::with_capacity(prepared.len());
            for (prompt, spawn_task_name) in prepared {
                results.push(take_agent_call(
                    &prompt,
                    spawn_task_name,
                    spawn,
                    &index_for_batch,
                    &yield_index_for_batch,
                    &journal_for_batch,
                    &pending_for_batch,
                )?);
            }
            Ok(results)
        },
    );
    engine.register_fn(
        "parallel",
        |_: Array| -> Result<Array, Box<EvalAltResult>> {
            Err(runtime_error(
                "parallel() was renamed to batch_agent(); it is sequential ordered agent work, not concurrent fan-out",
            ))
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

    let journal_for_pause = Rc::clone(&journal);
    let index_for_pause = Rc::clone(&index);
    let control_index_for_pause = Rc::clone(&control_index);
    engine.register_fn("pause", move || -> Result<(), Box<EvalAltResult>> {
        take_control(
            &index_for_pause,
            &control_index_for_pause,
            &journal_for_pause,
            ContinuationKind::Pause,
        )
    });
    let journal_for_await = Rc::clone(&journal);
    let index_for_await = Rc::clone(&index);
    let control_index_for_await = Rc::clone(&control_index);
    engine.register_fn("await_user", move || -> Result<(), Box<EvalAltResult>> {
        take_control(
            &index_for_await,
            &control_index_for_await,
            &journal_for_await,
            ContinuationKind::AwaitUser,
        )
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
) -> Result<
    (
        WorkflowEval,
        serde_json::Value,
        Option<ContinuationKind>,
        Option<String>,
    ),
    WorkflowSourceError,
> {
    if let Some(token) = find_control_token(&error) {
        return match token {
            ControlToken::Complete(value) => Ok((WorkflowEval::Completed, value, None, None)),
            ControlToken::Yield {
                instruction,
                kind,
                request_digest,
            } => Ok((
                WorkflowEval::Yielded { instruction },
                serde_json::Value::Null,
                Some(kind),
                Some(request_digest),
            )),
            ControlToken::Pause {
                kind,
                request_digest,
            } => Ok((
                WorkflowEval::Paused,
                serde_json::Value::Null,
                Some(kind),
                Some(request_digest),
            )),
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

fn take_journal_or_yield(
    index: &Rc<Cell<usize>>,
    yield_index: &Rc<Cell<usize>>,
    journal: &Rc<Vec<ContinuationRecord>>,
    kind: ContinuationKind,
    request: &serde_json::Value,
    instruction: &str,
    empty_error: &str,
) -> Result<HostCallResult, Box<EvalAltResult>> {
    let digest = request_digest(kind, request);
    match lookup(journal, index.get(), kind, &digest) {
        JournalLookup::Replay(result) => {
            bump(index);
            bump(yield_index);
            Ok(result)
        }
        JournalLookup::NeedWork => {
            if yield_index.get() >= MAX_WORKFLOW_YIELDS as usize {
                return Err(runtime_error(format!(
                    "workflow exceeded {MAX_WORKFLOW_YIELDS} yields"
                )));
            }
            if instruction.trim().is_empty() {
                return Err(runtime_error(empty_error));
            }
            Err(terminated(ControlToken::Yield {
                instruction: instruction.to_string(),
                kind,
                request_digest: digest,
            }))
        }
        JournalLookup::Diverged => Err(runtime_error(REPLAY_DIVERGENCE)),
    }
}

fn take_control(
    index: &Rc<Cell<usize>>,
    control_index: &Rc<Cell<usize>>,
    journal: &Rc<Vec<ContinuationRecord>>,
    kind: ContinuationKind,
) -> Result<(), Box<EvalAltResult>> {
    let digest = request_digest(kind, &control_request());
    match lookup(journal, index.get(), kind, &digest) {
        JournalLookup::Replay(_) => {
            bump(index);
            bump(control_index);
            Ok(())
        }
        JournalLookup::NeedWork => {
            if control_index.get() >= MAX_WORKFLOW_CONTROL_RESUMES as usize {
                return Err(runtime_error(format!(
                    "workflow exceeded {MAX_WORKFLOW_CONTROL_RESUMES} control resumes"
                )));
            }
            Err(terminated(ControlToken::Pause {
                kind,
                request_digest: digest,
            }))
        }
        JournalLookup::Diverged => Err(runtime_error(REPLAY_DIVERGENCE)),
    }
}

fn bump(index: &Rc<Cell<usize>>) {
    index.set(index.get().saturating_add(1));
}

fn host_call_dynamic(result: &HostCallResult) -> Dynamic {
    let mut map = Map::new();
    map.insert("ok".into(), Dynamic::from(result.ok));
    map.insert("text".into(), Dynamic::from(result.text.clone()));
    map.insert("error".into(), Dynamic::from(result.error.clone()));
    Dynamic::from(map)
}

fn take_agent_call(
    prompt: &str,
    spawn_task_name: Option<String>,
    spawn: SpawnBinding,
    index: &Rc<Cell<usize>>,
    yield_index: &Rc<Cell<usize>>,
    journal: &Rc<Vec<ContinuationRecord>>,
    pending_spawn: &Rc<RefCell<Option<String>>>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    if let Some(task_name) = spawn_task_name {
        if spawn != SpawnBinding::Available {
            return Err(runtime_error("stock spawn_agent is unavailable"));
        }
        let digest = request_digest(
            ContinuationKind::SpawnAgent,
            &spawn_request(prompt, &task_name),
        );
        match lookup(journal, index.get(), ContinuationKind::SpawnAgent, &digest) {
            JournalLookup::Replay(result) => {
                bump(index);
                bump(yield_index);
                return Ok(host_call_dynamic(&result));
            }
            JournalLookup::NeedWork => {
                if yield_index.get() >= MAX_WORKFLOW_YIELDS as usize {
                    return Err(runtime_error(format!(
                        "workflow exceeded {MAX_WORKFLOW_YIELDS} yields"
                    )));
                }
                if prompt.trim().is_empty() {
                    return Err(runtime_error("agent() requires a nonempty prompt"));
                }
                *pending_spawn.borrow_mut() = Some(task_name);
                return Err(terminated(ControlToken::Yield {
                    instruction: prompt.to_string(),
                    kind: ContinuationKind::SpawnAgent,
                    request_digest: digest,
                }));
            }
            JournalLookup::Diverged => return Err(runtime_error(REPLAY_DIVERGENCE)),
        }
    }
    take_journal_or_yield(
        index,
        yield_index,
        journal,
        ContinuationKind::Agent,
        &agent_request(prompt),
        prompt,
        "agent() requires a nonempty prompt",
    )
    .map(|result| host_call_dynamic(&result))
}
