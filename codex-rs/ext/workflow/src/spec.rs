//! Markdown workflow definitions.
//!
//! A workflow is a named sequence of host-owned steps. This is the HOW layer:
//! it is independent of `/goal` (why / until when) and is not Grok's `.rhai`
//! `WorkflowManager`.

use std::fmt;

/// Inclusive cap on steps in one definition.
pub const MAX_WORKFLOW_STEPS: usize = 32;
/// Inclusive cap on one step instruction.
pub const MAX_STEP_INSTRUCTION_CHARS: usize = 8_000;
/// Inclusive cap on the source document.
pub const MAX_WORKFLOW_SOURCE_CHARS: usize = 32_000;
/// Inclusive cap on the workflow name.
pub const MAX_NAME_CHARS: usize = 200;

/// One named step in a workflow definition.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub title: String,
    pub instruction: String,
}

/// Parsed workflow: a name plus at least one step.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowDefinition {
    pub name: String,
    pub steps: Vec<WorkflowStep>,
}

/// Why a workflow document could not be parsed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowParseError {
    Empty,
    TooLarge { actual: usize },
    TooManySteps { actual: usize },
    MissingSteps,
    InvalidName,
    InvalidStep { reason: String },
}

impl fmt::Display for WorkflowParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("workflow source is empty"),
            Self::TooLarge { actual } => write!(
                f,
                "workflow source is {actual} characters; max is {MAX_WORKFLOW_SOURCE_CHARS}"
            ),
            Self::TooManySteps { actual } => {
                write!(
                    f,
                    "workflow has {actual} steps; max is {MAX_WORKFLOW_STEPS}"
                )
            }
            Self::MissingSteps => f.write_str("workflow needs at least one step"),
            Self::InvalidName => f.write_str("workflow name is empty or too long"),
            Self::InvalidStep { reason } => f.write_str(reason),
        }
    }
}

impl std::error::Error for WorkflowParseError {}

/// Parse a markdown workflow.
///
/// `# Title` is the workflow name. `## Step` headings are steps; the body
/// until the next heading is the instruction. A document with no headings is
/// a single-step workflow titled `Work`.
pub fn parse_workflow_markdown(source: &str) -> Result<WorkflowDefinition, WorkflowParseError> {
    if source.chars().count() > MAX_WORKFLOW_SOURCE_CHARS {
        return Err(WorkflowParseError::TooLarge {
            actual: source.chars().count(),
        });
    }
    let source = source.trim();
    if source.is_empty() {
        return Err(WorkflowParseError::Empty);
    }

    let mut name = None;
    let mut pending_title: Option<String> = None;
    let mut pending_body = String::new();
    let mut steps = Vec::new();

    for line in source.lines() {
        if let Some(title) = heading(line, 1) {
            if name.is_none() && pending_title.is_none() && steps.is_empty() {
                pending_body.clear();
                name = Some(title);
                continue;
            }
            flush_step(&mut steps, pending_title.take(), &mut pending_body)?;
            pending_title = Some(title);
            continue;
        }
        if let Some(title) = heading(line, 2) {
            if pending_title.is_none() {
                pending_body.clear();
            } else {
                flush_step(&mut steps, pending_title.take(), &mut pending_body)?;
            }
            pending_title = Some(title);
            continue;
        }
        append_body_line(&mut pending_body, line);
    }
    flush_step(&mut steps, pending_title.take(), &mut pending_body)?;

    if steps.is_empty() {
        let instruction = pending_body.trim();
        if instruction.is_empty() {
            return Err(WorkflowParseError::MissingSteps);
        }
        steps.push(make_step(0, "Work", instruction)?);
        pending_body.clear();
    }

    let name = match name {
        Some(name) => name,
        None => steps
            .first()
            .map(|step| step.title.clone())
            .unwrap_or_else(|| "workflow".to_string()),
    };
    let name = validate_name(&name)?;
    uniquify_step_ids(&mut steps);
    Ok(WorkflowDefinition { name, steps })
}

fn heading(line: &str, level: u8) -> Option<String> {
    let trimmed = line.trim();
    let marker = match level {
        1 => "# ",
        2 => "## ",
        _ => return None,
    };
    let title = trimmed.strip_prefix(marker)?.trim();
    if title.is_empty() || title.starts_with('#') {
        return None;
    }
    Some(title.to_string())
}

fn append_body_line(body: &mut String, line: &str) {
    if !body.is_empty() {
        body.push('\n');
    }
    body.push_str(line);
}

fn flush_step(
    steps: &mut Vec<WorkflowStep>,
    title: Option<String>,
    body: &mut String,
) -> Result<(), WorkflowParseError> {
    let Some(title) = title else {
        return Ok(());
    };
    if steps.len() >= MAX_WORKFLOW_STEPS {
        return Err(WorkflowParseError::TooManySteps {
            actual: steps.len().saturating_add(1),
        });
    }
    let instruction = body.trim();
    let instruction = if instruction.is_empty() {
        title.as_str()
    } else {
        instruction
    };
    steps.push(make_step(steps.len(), &title, instruction)?);
    body.clear();
    Ok(())
}

fn make_step(
    index: usize,
    title: &str,
    instruction: &str,
) -> Result<WorkflowStep, WorkflowParseError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(WorkflowParseError::InvalidStep {
            reason: "step title is empty".to_string(),
        });
    }
    if title.chars().count() > MAX_NAME_CHARS {
        return Err(WorkflowParseError::InvalidStep {
            reason: format!("step title is longer than {MAX_NAME_CHARS} characters"),
        });
    }
    if instruction.chars().count() > MAX_STEP_INSTRUCTION_CHARS {
        return Err(WorkflowParseError::InvalidStep {
            reason: format!(
                "step instruction is longer than {MAX_STEP_INSTRUCTION_CHARS} characters"
            ),
        });
    }
    Ok(WorkflowStep {
        id: slugify(title, index),
        title: title.to_string(),
        instruction: instruction.to_string(),
    })
}

fn validate_name(name: &str) -> Result<String, WorkflowParseError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(WorkflowParseError::InvalidName);
    }
    Ok(name.to_string())
}

fn slugify(title: &str, index: usize) -> String {
    let mut slug = String::new();
    let mut prev_underscore = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_underscore = false;
        } else if !slug.is_empty() && !prev_underscore {
            slug.push('_');
            prev_underscore = true;
        }
    }
    let slug = slug.trim_end_matches('_');
    if slug.is_empty() {
        format!("step_{index}")
    } else {
        slug.to_string()
    }
}

fn uniquify_step_ids(steps: &mut [WorkflowStep]) {
    let mut seen = std::collections::HashSet::new();
    for step in steps.iter_mut() {
        let mut candidate = step.id.clone();
        let mut suffix = 2;
        while !seen.insert(candidate.clone()) {
            candidate = format!("{}_{suffix}", step.id);
            suffix += 1;
        }
        step.id = candidate;
    }
}
