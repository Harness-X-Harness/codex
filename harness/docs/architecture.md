# Harness architecture

This document owns host-owned Goal completion and verification, Goal HOW
continuation, the independent Rhai `/workflow` product, and the one-occupant
engine contract shared by Goal HOW and `/workflow`.

It does not own Mini Proxy routing, grants, credentials, accounting, stock
Multi-Agent V2 semantics, official OpenAI Codex releases, or Grok Provider
semantics.

Git owns source identity. Native `.github/workflows/harness.yml` checks and the
unique composition Story own acceptance evidence.

## Product model

One experimental flag owns the feature: `features.goal_host`.
It stays default-off.

| Layer | `goal_host` off | `goal_host` on |
| --- | --- | --- |
| Goal | stock persisted Goal; worker `update_goal` | same Goal persistence; host evaluates completion |
| Goal policy | `ModelCommit` | `HostEvaluate` with skeptic verification |
| Goal HOW | ordinary stock continuation | host-owned continuation using the shared engine slot |
| Workflow | absent | independent Rhai `/workflow` |
| Multi-Agent V2 | stock | unchanged stock child runtime |

`/goal` remains gated by stock `Goals`. `/workflow` is gated by `goal_host`.
The host policy is independent of `model_provider`.

## Host Goal authority

When `goal_host` is on:

- goal-owned turns do not expose worker `update_goal` as the completion
  authority;
- the host evaluates each goal-owned round;
- a completion candidate is verified by the Guardian skeptic panel;
- verification failure or evaluator failure pauses rather than falsely
  completing the Goal;
- repeated blocker verdicts may mark the Goal blocked under the implemented
  host policy;
- an active Goal continues through Goal HOW while it remains active.

Goal is not a Rhai program. Setting a Goal persists Goal state and starts or
queues Goal HOW; it does not compile the objective into generated Rhai.
Goal HOW does not call the `/workflow` journal or catalog.

## Engine occupancy

When `goal_host` is on, one Thread admits at most one active engine occupant:

- Goal HOW; or
- one user-started `/workflow` run.

The later start waits in FIFO order. Goal state may persist while Goal HOW
waits. A paused, complete, stopped, or failed occupant releases the slot.
Persisted/restored state must reconcile to at most one actual owner; failed
claims or persistence failures must not leave phantom ownership or
`HostIdleHold` state.

A waiting Goal HOW does not run host evaluation. Completing or failing an
independent `/workflow` never writes Goal `complete` or `blocked`.

Stock `ModelCommit` Goals do not claim or wait on this engine slot when
`goal_host` is off.

## `/workflow` contract

`/workflow` is a host-owned Rhai HOW VM. Resume re-evaluates the program from
the start and replays only identity-checked continuation records.

The stable host bindings are:

- `ask(instruction)`;
- `agent(prompt)` and `agent(prompt, opts)`;
- `batch_agent([...])`;
- `yield_budget()`;
- `write_scratch_file(name, content)` / `read_scratch_file(name)`;
- `pause()` / `await_user()`;
- `phase(title)` / `log(message)`;
- `fingerprint(text)` / `json_encode(value)`;
- `complete()` / `complete(value)`.

`parallel()` and generic `budget()` are not bindings.

### Host results

Result-bearing host calls expose actual host operation outcomes as
`{ ok, text, error }`. Successful empty text is still success. Host/client
control RPCs must not fabricate a successful host result.
`thread/workflow/advance` is only an optional non-fabricating control-plane
nudge.

Unrecoverable runtime, replay, or persistence failure is terminal `failed`.
Terminal failure releases occupancy and does not write Goal state.

### Replay and durability

Replay identity includes continuation kind plus the effective request. Control
continuations (`pause`, `await_user`) also bind to a stable source callsite.
A mismatched replay fails closed before new host work.

Result-bearing host yields and control resumes have separate bounded budgets.
Resource exhaustion is checked before starting host work when a result would
need to be persisted.

Workflow persistence is bounded and atomic. Restore rejects unsafe/corrupt
state. An in-flight host operation that cannot be durably reattached after a
process restart fails closed rather than being blindly retried.

Explicit resume intent survives waiting for the engine slot and is consumed
once when ownership becomes available.

Scratch and other non-journaled local effects are at-least-once across replay.
Authors must make such effects idempotent when repetition matters.

### Agent execution

Default `agent` work stays on the same Thread. Opt-in spawn maps to existing
stock Multi-Agent V2 and requires `"spawn": true` plus a non-empty
`task_name`.

Workflow owns the spawn request/outcome contract; the host injects the adapter
that maps it onto stock child-agent execution. Stock child state semantics stay
unchanged. The Workflow-side wait is event-driven and cancellation-aware; a
late child result cannot journal into a stopped, failed, or replaced run.
The child is not a second engine occupant.

`batch_agent` is ordered sequential work, not concurrent fan-out. Every item is
validated before host work starts. Unsupported option keys fail before host
work.

### Source and bounds

Workflow source may come from inline Rhai, an existing file, or the named
catalog. Project workflows live under `<root>/.codex/workflows/*.rhai`; user
workflows live under `$CODEX_HOME/workflows/*.rhai`.

Source reads, catalog discovery, persisted state, arguments, evaluator output,
and VM work are bounded. `eval` and `import` are disabled. Goal-write names are
rejected.

## App Server surface

The independent line adds experimental App Server v2 methods:

- `thread/workflow/get`;
- `thread/workflow/start`;
- `thread/workflow/advance`;
- `thread/workflow/stop`;
- `thread/workflow/resume`;
- notification `thread/workflow/updated`.

`ThreadWorkflow.status` is `active | paused | complete | waiting | failed`.
`ThreadWorkflow.result` is present and remains JSON `null` until the run stores
a result. `ThreadWorkflow.error` is `string | null` secret-safe terminal
failure text.

Stock `thread/goal/*` method names and payload shapes remain unchanged.
With `goal_host` off, `thread/workflow/*` is unavailable.

Workflow DTOs belong to the dedicated Workflow protocol module rather than the
stock Thread DTO module. Runtime Workflow state belongs to the Workflow
extension. Stock integration code should remain thin.

TUI `/workflow` and the Python SDK Workflow surfaces follow this same contract.

## Configuration and lifecycle

Thread eligibility is established by the thread's real origin and must not be
created retroactively by config reload. Internal sessions remain ineligible for
Host Goal/Workflow product behavior.

Turning HostEvaluate off releases Goal HOW ownership and returns Goals to stock
policy semantics. Disabling Workflow stops/pauses host execution safely and
releases Workflow ownership; re-enable does not invent hidden work.

Thread stop evicts process-local Workflow runtime state and waiters while
durable Workflow state and scratch remain available for later restore.

## Verification

Git owns the source SHA. There is no separate current-delivery ledger.
`.github/workflows/harness.yml` is the deterministic acceptance boundary. It
must cover:

- flag-off stock Goal compatibility;
- host Goal authority, evaluator, and skeptic transitions;
- Goal HOW continuation and config changes;
- Workflow replay, durability, restart, resume, and failure;
- both occupancy directions and restore/release behavior;
- stock Multi-Agent V2 spawn integration;
- same-Thread Turn ownership and stop lifecycle;
- App Server schema/protocol plus TUI and Python Workflow surfaces.

The unique composition Live Story is
[`host-goal-remains-distinct-from-independent-workflow.md`](./stories/host-goal-remains-distinct-from-independent-workflow.md).
Run it when a candidate changes relevant Thread/Turn lifecycle, extension
composition, Goal HOW, Workflow auto-resume, or engine ownership semantics.
Deterministic gates remain prerequisites and do not replace that composition
proof.

Do not add uncontrolled model-dependent Live requirements for Goal
`complete`, `blocked`, or internal skeptic votes when deterministic host seam
tests can prove those contracts reliably.

Harness acceptance does not depend on Grok release state, Grok Live, or Mini
governance.

## Non-goals

Do not add, solely under this architecture:

- a second Goal completion authority;
- Goal-as-Rhai;
- a third child-agent runtime;
- multiple concurrent Workflow runs on one Thread;
- true concurrent `parallel()` fan-out;
- Grok token/cost ledgers or `AgentResult` parity;
- a generic compatibility, migration, RPC, or service framework merely to
  reduce ordinary stock merge conflicts;
- a second source/version ledger beside Git.

For every change, preserve the ownership split: Goal answers why/until when,
HostEvaluate decides who may say done, `/workflow` expresses scripted HOW, and
stock Multi-Agent V2 remains the child execution system.
