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
- the acceptance runner did not resubmit model work to manufacture success.

## Partial success is not completion

- Workflow completion also marks the Goal `complete` or `blocked`.
- Goal HOW becomes active while the Workflow is still active.
- A goal-owned turn exposes worker `update_goal`.
- Only crate/App Server tests run and no real Thread composition is exercised.
- The runner retries or resubmits the semantic operation to manufacture
  success.

## Material failure boundaries

- Completing or failing `/workflow` must not write Goal `complete` or
  `blocked`.
- The runner starts one Workflow and sets one Goal while the Workflow is
  active. It submits at most one Workflow-driven model turn and does not call
  `thread/workflow/advance` to force progress.

## What this does not prove

This Story does not prove every Rhai binding, stock Multi-Agent V2, Mini
routing, Grok Provider behavior, host evaluator `complete`/`blocked`
trajectories, or internal skeptic votes. Those belong to deterministic tests
or other owning contracts.

The reverse occupancy direction (Goal HOW active, then Workflow starts and
waits) is deterministic coverage rather than another Live Story.

## Proof plan

### Preconditions

- Candidate Codex App Server has stock `Goals` and `features.goal_host = true`.
- Experimental App Server API is enabled.
- One disposable isolated Home.
- The candidate already passed `goal-host-checks`.
- One ordinary Provider path may satisfy at most one Workflow model yield.
- Do not use a ChatGPT stock-Goals Live Turn as the control.

### Proof-run invocation budget

The runner starts one Workflow and sets one Goal while the Workflow is active.
It does not resubmit model work. Product-internal continuation is not another
runner invocation.

### Secret-safe evidence

Record Story ID and candidate source identity, `goal_host` enablement,
Goal status after set and after Workflow completion, Workflow status after
Goal set and at terminal state, whether Goal HOW started while the Workflow
was active, whether any goal-owned turn exposed `update_goal`, whether
`thread/workflow/advance` was called, and runner submission count. Negative
evidence: no credentials, prompts, responses, raw traffic, Thread IDs, or
local paths.

## Deterministic prerequisites

On the same candidate revision, `goal-host-checks` must prove:

- flag-off stock Goals retain worker `update_goal` authority and Workflow RPCs
  are unavailable;
- Host Goal pursuit hides worker `update_goal` and host evaluation owns Goal
  completion;
- Workflow source starts, yields, auto-resumes, and completes;
- Workflow completion does not create or update Goal state;
- both FIFO occupancy directions;
- pause/stop/complete/failed release ownership as specified;
- restore/rejected-start paths do not create phantom ownership;
- host evaluator complete/blocked/paused transitions are correct at the
  deterministic App Server seam.

Those tests do not replace this Live composition.

## Rerun rule

Run this Story when a candidate materially changes Thread/Turn lifecycle,
extension lifecycle/composition, Goal HOW continuation, Workflow
yield/auto-resume, engine ownership/idle hold, or same-Thread Turn ownership
relevant to Workflow completion.

Do not rerun it only because a branch name, commit SHA, documentation file, or
unrelated implementation changed.

## Last proven

Live Then last proven on source `a10fe97e2`: start `/workflow`, set-goal while
that run is `active`, workflow `complete`, goal still `active`.
