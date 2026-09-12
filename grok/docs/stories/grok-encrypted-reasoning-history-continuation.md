# Grok encrypted reasoning survives full-history continuation

## User story

As a Grok user, I can continue a Grok Thread after a reasoning and tool Turn,
so the next Turn completes without losing or corrupting encrypted reasoning
history.

## Real path

```text
exact Grok artifact
  -> one Grok-bound Thread
  -> Turn N reasoning + dynamic tool continuation
  -> canonical durable history
  -> Turn N+1 on the same Thread
  -> visible semantic terminal result
```

## Acceptance

**Given** an exact artifact with the release-bundled Grok catalog and selected
release model, **when** Turn N reasons, calls the named dynamic tool, consumes a
fresh safe result that was not present in the submitted prompt, and completes,
and the runner then starts Turn N+1 on the same Thread, **then**:

- Turn N exposes completed reasoning and a completed call to the named dynamic
  tool;
- Turn N's visible terminal reply contains the fresh tool result;
- Turn N+1's visible terminal reply contains the same result without the
  runner supplying it again;
- the exact deterministic projection contract proves that canonical encrypted
  reasoning is copied unchanged into the next Provider request, while the Live
  run proves that real encrypted reasoning and history-dependent continuation
  occur on the same Thread; and
- the runner submits Turn N and Turn N+1 once each.

## Partial success is not completion

- Turn N completes but Turn N+1 is not started.
- The tool completes but Turn N has no final assistant reply.
- Turn N+1 uses another Thread or a reconstructed history authority.
- A runner resubmission succeeds after either required Turn failed.

## Material failure boundaries

- The runner does not resubmit either Turn.
- Continuation must stay on the same Thread and the same Grok Provider.

## What this does not prove

This Story does not prove every tool type, compaction, fork, Multi-Agent
behavior, optional Provider capabilities, Mini routing, or future Provider
availability.

## Proof plan

### Preconditions

- Exact artifact with the release-bundled Grok catalog.
- Deterministic Grok history-projection tests passed for that source.

### Proof-run invocation budget

One proof run contains Turn N and Turn N+1 on one Thread. Each required Turn
is started once.

### Secret-safe evidence

Record Story ID, source SHA, validation run, archive identity, Provider and
model labels, structural completion assertions, fresh-result equality
booleans, and runner Turn-submission count. Negative evidence: no runner
re-invocation, no other Thread or history authority, no credentials, prompts,
responses, raw traffic, encrypted bytes, or Thread identifiers.

## Stock compatibility control

Deterministic tests must prove that the same canonical reasoning item remains
unchanged under stock Provider projection, that its encrypted value reaches
the projected next request unchanged, and that Grok normalization changes only
the request copy. Live observation alone does not inspect Provider egress.

## Executable contract

`TestGrokEncryptedReasoningContinuation` in the repository-owned `grok/live`
Go module. Normal Grok delivery runs
`go test ./... -count=1 -timeout 30m -run '^TestGrok'` from `grok/live` against
the exact current-run Linux archive. `llm-go/codexsdk` is the SDK dependency,
not the acceptance owner.
