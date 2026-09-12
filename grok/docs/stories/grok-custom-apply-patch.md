# Grok custom `apply_patch` file edit

## User story

As a Grok user, I complete one workspace file edit through the packaged Grok
App Server on a Grok-bound Thread so the only offered file-edit tool is Codex
free-form custom `apply_patch`.

## Real path

```text
exact Grok release artifact
  -> isolated Home and workspace
  -> one Grok-bound Thread with shell disabled
  -> one Turn that must edit a seeded workspace file
  -> custom apply_patch / file_change
  -> workspace file shows the expected result
  -> Turn reaches terminal completed
```

This Story owns Grok catalog advertisement of Codex free-form `apply_patch`
and the user-visible file result on a packaged Grok Turn. It does not complete
a Mini transport Story or a Local Adapter `apply_patch` Story.

## Acceptance

**Given** the preconditions, **when** the acceptance runner submits one Grok
App Server Turn that must edit a seeded workspace file and the only offered
file-edit tool is custom `apply_patch`, **then**:

- the Turn reaches terminal `completed`;
- the workspace file shows the expected result;
- the persisted session records a custom `apply_patch` or equivalent
  `file_change` owned by that tool;
- no `command_execution`, `exec_command`, or apply-patch CLI path explains the
  edit; and
- the runner submits that semantic Turn once.

## Partial success is not completion

- The workspace file changes through shell, `exec_command`, `write_stdin`, or
  the apply-patch CLI.
- The catalog advertises free-form `apply_patch` but the Turn never offers it.
- Deterministic projection or snapshot gates pass without the packaged file
  result.
- A Local Adapter or Mini Responses result is reported as this Story's
  evidence.
- A later repeated Turn is accepted after the first attempt failed.

## Material failure boundaries

- The runner does not resubmit a failed or incomplete Turn.
- No fallback to another Provider, product, or file-edit tool can complete
  this Story.

## What this does not prove

This Story does not prove Mini dialect rewriting, Local Adapter projection,
Gateway function-calling capability, same-Thread continuation, resume, fork,
compaction, child inheritance of `apply_patch`, another custom tool, or every
Grok model.

## Proof plan

### Preconditions

- Artifact source SHA, archive identity, and checksum are known.
- Native Grok Tool Plan, flat-projection, and model-visible request tests
  passed for that source.
- The release-bundled Grok catalog advertises free-form `apply_patch`.
- Isolated `CODEX_HOME` and workspace. `shell_tool` is disabled for the proof
  Turn.

### Proof-run invocation budget

One Grok App Server Turn. The runner does not resubmit after a missing
custom-tool item, a shell fallback, or a failed terminal result.

### Secret-safe evidence

Record Story ID, source SHA, validation run, archive identity, whether
`shell_tool` was disabled, whether the catalog advertised free-form
`apply_patch`, custom-tool or `file_change` presence class, workspace file
result class, Turn terminal class, and runner submission count. Negative
evidence: no runner re-invocation, no fallback outside Grok, no shell or
apply-patch CLI explanation, no Mini or Local Adapter result used as proof.

## Stock compatibility control

A ChatGPT-bound Thread keeps the upstream Tool Plan. Deterministic stock tests
at the same App Server and request boundaries must pass for the frozen source.
This Story does not perform a ChatGPT live Turn.

## Executable contract

`TestGrokCustomApplyPatch` in `ronhuafeng/llm-go` `codexsdk`. Direct
`go test . -count=1 -timeout 30m -run '^TestGrok'` against the Linux archive.

## Last proven

Release run
[34027180997](https://github.com/Harness-X-Harness/codex/actions/runs/34027180997)
on 2026-09-06 for published source `86b8ac4be290ea1c0136123a52c7463739074608`.
