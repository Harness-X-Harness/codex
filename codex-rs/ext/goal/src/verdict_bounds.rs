//! Local Host-owned bounds for evaluator and skeptic verdict strings.
//!
//! Provider/schema enforcement is not a memory bound. These caps reject
//! oversize instead of truncating a verdict into validity.

/// Inclusive UTF-8 byte cap on one streamed evaluator completion.
pub const EVALUATOR_OUTPUT_MAX_BYTES: usize = 16 * 1024;
/// Inclusive character cap on evaluator and skeptic `evidence`.
pub const GOAL_VERDICT_EVIDENCE_MAX_CHARS: usize = 2_048;
/// Inclusive character cap on evaluator and skeptic `next_step`.
pub const GOAL_VERDICT_NEXT_STEP_MAX_CHARS: usize = 512;
/// Inclusive character cap on evaluator `blocker_key`.
pub const GOAL_VERDICT_BLOCKER_KEY_MAX_CHARS: usize = 64;

/// Append streamed evaluator text if it stays within the local byte cap.
///
/// On overflow the accumulator is cleared so the oversized stream is not kept.
pub fn append_evaluator_output_text(
    acc: &mut String,
    text: &str,
) -> Result<(), EvaluatorOutputLimit> {
    if acc.len().saturating_add(text.len()) > EVALUATOR_OUTPUT_MAX_BYTES {
        acc.clear();
        return Err(EvaluatorOutputLimit);
    }
    acc.push_str(text);
    Ok(())
}

/// Streamed evaluator output exceeded [`EVALUATOR_OUTPUT_MAX_BYTES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvaluatorOutputLimit;

impl EvaluatorOutputLimit {
    pub fn message(self) -> &'static str {
        "goal evaluator output exceeded the local byte cap"
    }
}

pub(crate) fn field_exceeds_char_cap(value: &str, max_chars: usize) -> bool {
    value.chars().count() > max_chars
}
