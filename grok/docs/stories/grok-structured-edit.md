# Grok structured exact-match file edit

## User story and real path

As a Grok user, I complete one workspace file edit through the packaged Grok
App Server on a Grok-bound thread with the structured `structured_edit` function.

Exact Grok release artifact -> isolated home and workspace -> one Grok-bound
thread with shell disabled -> closed structured-edit advertisement -> one edit
turn -> exact file bytes -> same-thread history continuation -> terminal reply
without another edit. The offered edit tool is `structured_edit`; the packaged
fixture does not advertise `apply_patch`.

The product projects the allowed structured-edit function into Grok requests and
replays its paired result through canonical history. The controlled Integration
owner below exercises that real Codex path, including actual filesystem effects.
Offline observer tests validate evidence handling; Rust tests own editor safety.
Neither tier establishes acceptance by the real xAI backend or provenance of an
unidentified package. A packaged Live claim requires its own identified run.

## Acceptance

Given the preconditions, when the runner submits one Grok App Server turn that
must edit a seeded workspace file, followed once by a continuation on that same
thread, then:

- The first turn reaches terminal `completed`.
- Durable history records exactly one exact `structured_edit` identity.
- One `FileChange` completes for the intended fixture and call.
- The fixture has the expected bytes and SHA-256.
- No `apply_patch`, command execution, shell, Python, heredoc or `sed` path
  explains the edit.
- The continuation reaches terminal `completed` on the same thread.
- Its last `/responses` request replays the matching structured function call
  and output, and the backend accepts that request with HTTP 2xx.
- The continuation makes no new edit or command call and preserves the bytes.
- The runner submits each semantic turn once.

The declined-approval control requires one structured-edit attempt, the matching
file-change approval declined by the runner, a declined file change, no completed
or alternative edit, and unchanged fixture bytes. It uses `untrusted` approval;
`on-request` with unrestricted workspace writes does not establish an approval
decision. Missing, failed, interrupted or unrelated terminal evidence never
completes the scenario.

## Partial success and failure boundaries

Wrong provider, model, thread or turn; absent or repeated exact invocation;
unpaired function output; missing, failed or mismatched file change; successful
text without actual file effects; changed bytes after decline; or a repeated edit
during continuation cannot complete the Story. A near-match tool name or a
caller-supplied boolean is insufficient evidence. No runner resubmission or
fallback to another provider, product or editing tool can repair that run.

The packaged advertisement, durable-history, and accepted outbound replay
requirements remain separate from observing a raw App Server call/output pair.
The Live observer reads settled public history but does not capture outbound
`/responses` traffic or scan session storage. The controlled Integration test
independently checks the actual projected declaration and accepted paired HTTP
replay. Real-backend acceptance still requires identified Live evidence; observer
success alone cannot silently satisfy the outbound-request requirement.

## Proof plan

### Preconditions and invocation budget

- Exact artifact digest, source revision, harness revision, target, environment,
  model and observation time; caller-established package provenance.
- Same-source Linux native editor, real Local/Remote and Code Mode tests, plus
  affected stock `apply_patch` controls. Required native verification is Linux;
  distribution/platform capabilities are not removed.
- Release-bundled catalog advertises `structured_edit_tool_type = exact_match`
  and does not advertise freeform `apply_patch` for this fixture.
- Same-source Grok tool projection and history composition tests.
- Isolated home/workspace and shell disabled.

The main scenario starts one process, initializes once, creates one thread and
submits exactly two semantic turns. Decline, pinned-model and `replace_all`
controls each submit one turn in their own isolated invocation. Thread reads and
approval replies observe those invocations; they are not new turns. No missing
evidence triggers an automatic rerun. This budget does not impose a new global
same-provider transport retry policy.

### Bounded scenario evidence

The fixture enables the model capability and seeds a controlled CRLF file
without a final newline. Its observer correlates exact function identity and
arguments, paired output, file-change identity/status/path, approval identity,
matching terminal turns and one settled `thread/read` per turn. It compares
actual bytes after each turn and returns only subject metadata, fixed stage
labels, counts, booleans and fixture SHA-256 values. Opaque IDs and transient
arguments/output are used only for in-memory correlation. Frames retain the
common harness bounds; edit observations additionally cap event counts, argument
and output sizes. Errors contain no raw prompts, outputs, paths or traffic.

The [current failure-evidence contract](../../TESTING.md#live-failure-evidence)
owns the bounded, secret-safe observations returned by this runner. Partial
observations do not claim Story completion or assign defect ownership.

### Executable owners and evidence tiers

- [`StructuredEdit` and its decline, pinned and replace-all entries](../../live/structured_edit.go)
  implement the callable scenarios. The primary entry performs the continuation;
  each control has its own isolated invocation.
- [`TestStructuredEditScenarios` and `TestStructuredEditRejectsFalseEvidence`](../../live/structured_edit_test.go)
  use an effectful fake stdio server. [`TestSDKEditObserverPositiveStory` and
  negative controls](../../live/sdk_edit_observer_test.go) validate correlation,
  authorization, terminal evidence and exact effects. These are offline observer
  contracts, not the product editor.
- [`TestStructuredEditFixture`](../../tests/integration/structured_edit_test.go)
  executes real Codex against controlled HTTP for continuation, pinned, declined
  approval and replace-all. It checks projected `structured_edit` declarations,
  exact model binding, paired call/result replay, actual fixture bytes/hash,
  authorization and the same-thread follow-up. Its backend supplies the calls and
  accepts requests; this does not prove real xAI acceptance.
- [Rust structured-edit suites](../../../codex-rs/core/tests/suite/structured_edit.rs)
  own exact matching/cardinality, stale-content rejection, approval, hooks,
  cancellation and committed filesystem effects. Conditional mutation keeps
  stock sandbox and effect ownership through settlement; an uncertain/lost reply
  must not cause a repeated write or publish an uncommitted delta.
- [`TestLive/structured-edit`, `structured-edit-declined`,
  `structured-edit-pinned` and `structured-edit-all`](../../tests/live/live_test.go)
  execute the catalog-selected real-backend owners. Their existence is not a
  passing run. Commands and package/subject requirements are in [TESTING](../../TESTING.md).

The pinned control verifies the previous-model binding. The `replace_all` control
seeds two occurrences and requires one call with the genuine JSON boolean `true`,
paired output and exact resulting bytes. Neither replaces Rust cardinality tests.

## Stock control and limits

A ChatGPT-bound thread retains its stock tool plan, including `apply_patch`.
Same-source deterministic stock tests remain required; this Story adds no
ChatGPT live turn. It does not prove Mini transport, Local Adapter/Gateway
projection, resume, fork, compaction, child inheritance, every model, or future
backend availability. Offline observer success alone cannot establish native
composition; controlled Integration success cannot establish real-backend Story
completion.
