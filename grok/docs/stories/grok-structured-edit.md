# Grok structured exact-match file edit

## User story and real path

As a Grok user, I complete one workspace file edit through the packaged Grok
App Server on a Grok-bound thread with the structured `structured_edit` function.

Exact Grok release artifact -> isolated home and workspace -> one Grok-bound
thread with shell disabled -> closed structured-edit advertisement -> one edit
turn -> exact file bytes -> same-thread history continuation -> terminal reply
without another edit. The offered edit tool is `structured_edit`; the packaged
fixture does not advertise `apply_patch`.

This preserves the accepted packaged-Grok boundary. An independently generated
harness catalog, a fake stdio server, or provider-neutral Rust editor tests do
not establish this Story. C4b restores deterministic scenario support and the
provider-neutral editor implementation; actual Grok tool/HTTP composition is
#322, shipped catalog/profile is #329, package invocation is #339, and authorized
backend execution is #331. The current Grok projection rejects nonempty tools.
This Story remains unproven for the carry-forward candidate.

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
completes the restored deterministic scenario.

## Partial success and failure boundaries

Wrong provider, model, thread or turn; absent or repeated exact invocation;
unpaired function output; missing, failed or mismatched file change; successful
text without actual file effects; changed bytes after decline; or a repeated edit
during continuation cannot complete the Story. A near-match tool name or a
caller-supplied boolean is insufficient evidence. No runner resubmission or
fallback to another provider, product or editing tool can repair that run.

The packaged advertisement, durable-history, and accepted outbound replay
requirements remain separate from observing a raw App Server call/output pair.
The C4b runner does not observe an outbound `/responses` request or scan session
storage, and its success cannot silently satisfy either requirement.

## Proof plan

### Preconditions and invocation budget

- Exact artifact digest, source revision, harness revision, target, environment,
  model and observation time; caller-established package provenance.
- Same-source Linux native editor, real Local/Remote and Code Mode tests, plus
  affected stock `apply_patch` controls. Required native verification is Linux;
  distribution/platform capabilities are not removed.
- Release-bundled catalog advertises `structured_edit_tool_type = exact_match`
  and does not advertise freeform `apply_patch` for this fixture.
- Relevant #322 Grok tool projection and history composition prerequisites.
- Isolated home/workspace and shell disabled.

The main scenario starts one process, initializes once, creates one thread and
submits exactly two semantic turns. Decline, pinned-model and `replace_all`
controls each submit one turn in their own isolated invocation. Thread reads and
approval replies observe those invocations; they are not new turns. No missing
evidence triggers an automatic rerun. This budget does not impose a new global
same-provider transport retry policy.

### Bounded scenario evidence

The C4b fixture enables the model capability and seeds a controlled CRLF file
without a final newline. Its observer correlates exact function identity and
arguments, paired output, file-change identity/status/path, approval identity,
matching terminal turns and one settled `thread/read` per turn. It compares
actual bytes after each turn and returns only subject metadata, fixed stage
labels, counts, booleans and fixture SHA-256 values. Opaque IDs and transient
arguments/output are used only for in-memory correlation. Frames retain the
common harness bounds; edit observations additionally cap event counts, argument
and output sizes. Errors contain no raw prompts, outputs, paths or traffic.

The historical Story declared: GREEN records nothing; RED retains redacted
session JSONL and rejected-request key-path shapes, backend status and redacted
error text for seven days. That retention requirement has not been implemented
or silently replaced by C4b. Before eventual packaged Live acceptance, its owner
must review how to retain the required facts under the current bounded,
secret-safe evidence policy. C4b neither collects those raw artifacts nor claims
that its smaller return value completes the historical retention requirement.

### Executable source and deterministic support

`StructuredEdit`, `StructuredEditApprovalDeclined`,
`StructuredEditPinnedPreviousModel`, and `StructuredEditReplaceAll` in
`grok/live` restore the retained callable outcomes through the existing shared
App Server runner. They introduce no backend opt-in or automatic invocation.

`TestStructuredEditScenarios` and `TestStructuredEditRejectsFalseEvidence`
exercise an effectful fake stdio server. The server really changes or preserves
the seeded fixture for its modeled outcomes; it is not the product editor.
The consumed argument tests require exact string fields and a genuine JSON
boolean for `replace_all`. Actual advertisement and accepted HTTP replay remain
unproven C7/#322 obligations, beyond this App Server observer.

The pinned control selects the inherited `grok-4.6` harness model and verifies the
thread binding. Eventual package acceptance still requires the shipped primary
and previous-model catalog checks and that pinned slug on `/responses`.
The `replace_all` control seeds two occurrences and requires one call with the
JSON boolean `true`, paired output and the exact resulting bytes. Rust tests own
normative matching/cardinality; this control does not replace them.

The historical `TestGrokStructuredEditWireContract`, `TestGrokStructuredEdit`,
`TestGrokStructuredEditApprovalDeclined`, pinned-model and replace-all gates are
retained outcomes, not currently runnable backend test entrypoints. Their package,
advertisement, durable-history and accepted-HTTP obligations remain with the
owners above. Current scheduling/invocation authority is the
[carry-forward roadmap](../rust-v0.158.0-carry.md); no retired release harness is
restored.

## Stock control and limits

A ChatGPT-bound thread retains its stock tool plan, including `apply_patch`.
Same-source deterministic stock tests remain required; this Story adds no
ChatGPT live turn. It does not prove Mini transport, Local Adapter/Gateway
projection, resume, fork, compaction, child inheritance, every model, or future
backend availability. The deterministic harness and native Basic controlled-HTTP
test cannot establish native structured Grok composition or Story PROVEN.
