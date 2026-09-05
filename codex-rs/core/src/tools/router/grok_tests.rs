//! Grok-owned tests for the flat tool projection as wired through `ToolRouter`.
//!
//! The projection itself lives in `codex_tools::flat_projection`; these tests
//! exercise it through the router's round trip and pin the stock shapes the
//! Grok graft depends on.

#[path = "flat_projection_tests.rs"]
mod flat_projection;

#[path = "seam_pins_tests.rs"]
mod seam_pins;
