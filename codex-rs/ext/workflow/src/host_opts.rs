//! Strict `/workflow` option keys for `agent` and `batch_agent`.

use rhai::Dynamic;
use rhai::EvalAltResult;
use rhai::Map;
use rhai::Position;

const AGENT_KEYS: &[&str] = &["spawn", "task_name"];
const BATCH_KEYS: &[&str] = &["prompt", "spawn", "task_name"];

fn runtime_error(message: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        Dynamic::from(message.into()),
        Position::NONE,
    ))
}

fn reject_unknown_keys(opts: &Map, allowed: &[&str], what: &str) -> Result<(), Box<EvalAltResult>> {
    for key in opts.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(runtime_error(format!(
                "{what} does not accept option `{key}`"
            )));
        }
    }
    Ok(())
}

/// Validate `agent(prompt, opts)` and return a spawn `task_name` when requested.
pub fn spawn_task_name(opts: Option<&Map>) -> Result<Option<String>, Box<EvalAltResult>> {
    let Some(opts) = opts else {
        return Ok(None);
    };
    reject_unknown_keys(opts, AGENT_KEYS, "agent()")?;
    spawn_task_name_from_validated(opts, "agent()")
}

/// Validate one `batch_agent` item and return its prompt plus optional spawn name.
pub fn batch_item_prompt_and_spawn(
    opts: &Map,
) -> Result<(String, Option<String>), Box<EvalAltResult>> {
    reject_unknown_keys(opts, BATCH_KEYS, "batch_agent()")?;
    let prompt = match opts.get("prompt") {
        Some(value) => value
            .clone()
            .into_string()
            .map_err(|_| runtime_error("batch_agent() requires a nonempty prompt"))?,
        None => return Err(runtime_error("batch_agent() requires a nonempty prompt")),
    };
    if prompt.trim().is_empty() {
        return Err(runtime_error("batch_agent() requires a nonempty prompt"));
    }
    let spawn = spawn_task_name_from_validated(opts, "batch_agent()")?;
    Ok((prompt, spawn))
}

fn spawn_task_name_from_validated(
    opts: &Map,
    what: &str,
) -> Result<Option<String>, Box<EvalAltResult>> {
    let spawn = match opts.get("spawn") {
        None => None,
        Some(value) => match value.clone().try_cast::<bool>() {
            Some(flag) => Some(flag),
            None => {
                return Err(runtime_error(format!(
                    "{what} option `spawn` must be a boolean"
                )));
            }
        },
    };
    let task_name = match opts.get("task_name") {
        None => None,
        Some(value) => {
            let name = value.clone().into_string().map_err(|_| {
                runtime_error(format!(
                    "{what} option `task_name` must be a nonempty string"
                ))
            })?;
            Some(name)
        }
    };
    match (spawn, task_name) {
        (Some(true), Some(name)) if !name.trim().is_empty() => Ok(Some(name)),
        (Some(true), _) => Err(runtime_error(format!(
            "{what} spawn requires a nonempty task_name"
        ))),
        (_, Some(_)) => Err(runtime_error(format!(
            "{what} option `task_name` requires `\"spawn\": true`"
        ))),
        (Some(false) | None, None) => Ok(None),
    }
}
