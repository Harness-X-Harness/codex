# Grok structured exact-match file edit

## User story

As a Grok user, I complete one workspace file edit through the packaged Grok
App Server on a Grok-bound Thread so the model-visible editor is the structured
`structured_edit` function, not freeform `apply_patch`.

## Real path

```text
exact Grok release artifact
  -> isolated Home and workspace
  -> one Grok-bound Thread with shell disabled
  -> advertised tools include a closed structured_edit function and no apply_patch
  -> one Turn that must edit a seeded workspace file
  -> exactly one structured_edit invocation and a completed FileChange
  -> workspace file shows the expected bytes and hash
  -> same-Thread continuation replays function_call / function_call_output
  -> Turn 2 completes without repeating the edit
```

This Story owns Grok catalog advertisement of structured exact-match editing
and the user-visible file result on a packaged Grok Turn. It does not complete
a Mini transport Story or an OpenAI `apply_patch` Story.

## Acceptance

**Given** the preconditions, **when** the acceptance runner submits one Grok
App Server Turn that must edit a seeded workspace file and the only offered
file-edit tool is structured `structured_edit`, then one follow-up Turn on the
same Thread, **then**:

- the first Turn reaches terminal `completed`;
- durable history records exactly one `structured_edit` identity;
- a `FileChange` completes;
- the workspace file has the expected final bytes and SHA-256;
- no `apply_patch`, `command_execution`, `exec_command`, Python, heredoc, or
  `sed` path explains the edit;
- Turn 2 on the same Thread reaches terminal `completed`;
- the last `/responses` request replays the structured function call/output
  and is accepted (HTTP 2xx); and
- the runner submits each semantic Turn once.

## Partial success is not completion

- `thread_bound_to_grok`: the Thread is not bound to the Grok Provider.
- `turn_completed`: Turn 1 does not reach terminal `completed`.
- `structured_edit_exactly_once`: the proof counted zero or more than one
  exact `structured_edit` identity.
- `apply_patch_absent`: an `apply_patch` identity was observed.
- `command_execution_absent`: a shell or exec path explained the edit.
- `file_change_completed`: a completed `FileChange` is missing, or a declined
  or failed change is present.
- `workspace_file_verified`: the fixture bytes or hash are not the expected
  result.
- `continuation_turn_completed`: Turn 2 does not reach terminal `completed`
  on the same Thread.
- `structured_edit_replayed_accepted`: the last `/responses` request does not
  replay paired structured-edit `function_call` / `function_call_output` with
  HTTP 2xx.
- `continuation_did_not_repeat_edit`: Turn 2 invoked `structured_edit`,
  `apply_patch`, or a command.

## Material failure boundaries

- The runner does not resubmit a failed or incomplete Turn.
- A boolean or `strings.Contains(name, ...)` observation cannot complete this
  Story.
- No fallback to another Provider, product, or file-edit tool can complete
  this Story.

## What this does not prove

This Story does not prove Mini dialect rewriting, Local Adapter projection,
Gateway function-calling capability, resume, fork, compaction, child
inheritance of `structured_edit`, `replace_all` matching rules, or every
Grok model. Cardinality and matching rules belong to the Rust
`structured_edit` tests.

## Proof plan

### Preconditions

- Native structured-edit and stock `apply_patch` Cargo tests passed for that
  source.
- The release-bundled Grok catalog advertises `structured_edit_tool_type =
  exact_match` and does not advertise freeform `apply_patch`.
- Isolated `CODEX_HOME` and workspace. `shell_tool` is disabled for the proof
  Turns.

### Proof-run invocation budget

One Grok App Server Turn for the edit and one continuation Turn on the same
Thread. The runner does not resubmit after a missing structured-edit item, a
shell fallback, or a failed terminal result.

### Secret-safe evidence

GREEN records nothing. A RED run preserves, for 7 days, the redacted session
JSONL and the key-path shape of the rejected requests with the backend status
and redacted error text; no prompt text, model output, credential, or Thread ID
is recorded.

## Stock compatibility control

A ChatGPT-bound Thread keeps the upstream Tool Plan, including stock
`apply_patch`. Deterministic stock tests at the same App Server and request
boundaries must pass for the frozen source. This Story does not perform a
ChatGPT live Turn.

## Executable contract

`TestGrokStructuredEditWireContract`, `TestGrokStructuredEdit`, and
`TestGrokStructuredEditApprovalDeclined` in `grok/live`. Scheduling is
[`release.md`](../release.md).
