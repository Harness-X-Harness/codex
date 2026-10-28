# Grok workspace file edit

## User story and real path

As a Grok user, I ask the packaged Grok App Server to update a workspace file,
then confirm the result on the same thread. The fixture uses a fresh home and
workspace, a Grok-bound model, and the existing structured-edit capability.
Shell remains disabled; this Story does not broaden product capabilities.

## Acceptance

After a natural request to replace the seeded text while preserving everything
else, the matching turn completes successfully with a final response, and the
actual file has the expected bytes. The main scenario asks a same-thread
follow-up to check the result; it must complete and leave those bytes unchanged.
The pinned-model control checks the same outcome with the pinned model. The
replace-all control requires both seeded occurrences to change, not a particular
argument spelling or number of calls.

Correct results do not fail because the model uses different replacement
boundaries, makes multiple edits, or uses available auxiliary tools. Raw internal
call/output records and an exact count of file-change items are not acceptance
requirements. Tool counts remain diagnostic, not a script for the model.

The declined-approval control uses `untrusted` approval and requires at least one
matching file-change approval to be declined. The file must remain unchanged;
a completed file change after denial cannot pass even if the final bytes look
unchanged. A missing or unrelated approval is not proof of denial. Missing,
failed, interrupted or unrelated terminal evidence cannot complete any control.

## Proof plan

The runner binds the selected package and model, seeds a CRLF file without a
final newline, and submits each semantic request once. The main scenario uses
two turns on one thread; decline, pinned and replace-all each use one turn in a
fresh invocation. Missing evidence does not trigger a paid automatic rerun.

Bounded public App Server observations establish turn completion, eligible final
responses and approval decisions. A settled thread read is correlated to the
submitted turn. The runner reads the actual fixture bytes after each turn and
again after process cleanup. Successful text alone, wrong bytes, continuation
mutation, or mutation after shutdown cannot pass. Evidence retains fixed stage
labels, counts, booleans and fixture SHA-256 values, not prompts or file paths.
The [failure-evidence contract](../../TESTING.md#live-failure-evidence) owns the
secret-safe failure report.

### Executable owners and evidence tiers

- [`StructuredEdit` and its controls](../../live/structured_edit.go) implement
  these user-visible outcome gates.
- [`TestStructuredEditScenarios` and negative controls](../../live/structured_edit_test.go)
  use an effectful fake stdio server. The [SDK observer tests](../../live/sdk_edit_observer_test.go)
  cover alternate execution paths, terminal correlation and denied approvals.
  These are offline oracle tests, not real-backend success.
- [`TestStructuredEditFixture`](../../tests/integration/structured_edit_test.go)
  executes real Codex against controlled HTTP. Its scripted backend separately
  verifies structured tool advertisement, exact model binding, canonical paired
  call/result replay and real file effects. Those deterministic protocol checks
  do not constrain how a real model must solve the Live task.
- [Rust editor suites](../../../codex-rs/core/tests/suite/structured_edit.rs)
  retain exact matching/cardinality, stale-content rejection, approval, hooks,
  cancellation and committed filesystem safety coverage.
- [`TestLive` structured-edit entries](../../tests/live/live_test.go) exercise
  the real backend. Their existence and offline passes are not a passing Live run.
  See [TESTING](../../TESTING.md) for package and invocation requirements.

## Scope and limits

Stock ChatGPT tool plans, including `apply_patch`, are unchanged. This repair adds
no ChatGPT Live turn and does not establish resume, fork, compaction, child
inheritance, every model or future backend availability. Protocol composition,
controlled native execution and real-backend acceptance remain distinct claims.
