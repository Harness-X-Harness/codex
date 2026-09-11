# Host-owned goal stays distinct from an independent workflow

## User story

As a Codex user with `goal_host` enabled, I can start an independent Rhai
`/workflow` and set a thread Goal on the same Thread. The Goal persists while
Goal HOW waits for the engine. Completing the Workflow does not complete the
Goal.

This Story belongs to [Harness architecture](../architecture.md).

## Real path

```text
one idle Thread with goal_host on
  -> start independent Rhai /workflow
  -> set a persisted Goal while that Workflow is active
  -> host auto-resumes the Workflow to complete
  -> Goal remains active
```

## Acceptance

**Given** one idle Thread,
**when** the user starts an independent Rhai Workflow, then sets a persisted
Goal while that Workflow is `active`, and the host auto-resumes the Workflow to
`complete`,
**then**:

- the Workflow is visible as a separate HOW surface and owns the engine while
  active;
- the Goal is persisted and remains `active` while Goal HOW waits;
- Goal HOW does not become active while the Workflow owns the engine;
- any goal-owned turn that later starts does not expose worker `update_goal`;
- the Workflow reaches `complete` through host auto-resume after its yield;
- `thread/workflow/advance` is not required and cannot supply a host result;
- after Workflow completion, the Goal is still `active`;
- Workflow `complete()` did not write Goal `complete` or `blocked`; and
- host continuation does not require a client to resubmit model work to
  manufacture semantic success.

## Partial success is not completion

- Workflow completion also marks the Goal `complete` or `blocked`.
- Goal HOW becomes active while the Workflow is still active.
- A goal-owned turn exposes worker `update_goal`.
- Only isolated extension/unit tests run and no App Server Thread composition
  is exercised.
- Progress requires client resubmission or fabricated success through
  `thread/workflow/advance`.

## Material failure boundaries

- Completing or failing `/workflow` must not write Goal `complete` or
  `blocked`.
- Goal HOW and `/workflow` must not become concurrent engine occupants on one
  Thread.
- Control RPCs must not fabricate host results or require semantic model-work
  resubmission to reach the accepted state.

## What this does not prove

This Story does not prove every Rhai binding, stock Multi-Agent V2, Mini
routing, Grok Provider behavior, host evaluator `complete`/`blocked`
trajectories, or internal skeptic votes. Those belong to their owning native
contracts.

## Executable contract

`.github/workflows/harness.yml` is the canonical Harness acceptance path and
runs the owning native App Server integration suites directly.

The stable composition in this Story is covered by:

- `codex-rs/app-server/tests/suite/v2/thread_workflow.rs`, including
  `active_workflow_hold_blocks_goal_idle`,
  `goal_host_set_then_independent_workflow_leaves_goal_active`,
  `workflow_complete_persists_this_run_result_without_writing_goal`,
  `workflow_yield_turn_auto_advances_to_complete`, and
  `workflow_advance_is_optional_nudge`;
- `codex-rs/app-server/tests/suite/v2/thread_workflow_occupancy.rs`, including
  restore/reconcile of an active Workflow with an active Goal and
  `resume_of_pause_waits_then_completes_without_a_second_resume`; and
- `codex-rs/app-server/tests/suite/v2/thread_goal_host.rs` for host-owned Goal
  evaluator completion, blocked, and pause transitions.

These suites exercise actual App Server Thread/Goal/Workflow composition using
controlled host and Responses seams. Story Markdown describes the product
claim; it is not executable acceptance input. There is no separate
provider-dependent Harness Live authority for these deterministic Harness-owned
semantics.
