# Grok child agent inherits parent Provider authority

## User story

As a Grok user, I can delegate one bounded task from a Grok-bound parent
Thread to a child agent, receive the child's result, and complete the parent
task without leaving the Grok Provider boundary.

## Real path

```text
one observably Grok-bound parent Thread
  -> natural user-level delegation
  -> a runtime-created default/full-history child completes a bounded task
  -> the child result returns to the parent
  -> the parent uses that result and completes
```

## Acceptance

**Given** one parent Thread that visibly reports its Grok Provider binding,
**when** the user submits one natural task that requires bounded delegation,
**then**:

- at least one runtime-created default/full-history child is visible under the
  parent and reports the expected Grok Provider;
- the deterministic stock inheritance contract establishes that an
  unoverridden natural spawn uses the expected default/full-history child model
  class;
- a delegated child produces a fresh, safe bounded result that was not known to
  the parent when the user task was submitted;
- that result is delivered to the parent through the stock collaboration
  lifecycle;
- the parent reaches a visible terminal result that contains the same fresh
  child result; and
- the accepted child and parent result cannot be supplied by another Provider
  or by an acceptance-runner replay.

## Partial success is not completion

- The parent completes without consuming a valid delegated result.
- A result from another Provider or a second lookup path is accepted as the
  Grok child result.
- Only an internal child constructor or inheritance fixture is exercised.

## Material failure boundaries

- The runner submits one natural user task and does not resubmit it.
- No fallback to another Provider completes this Story.
- Child count, collaboration-tool calls, Provider responses, wait lifecycle
  events, ordering, and duration are diagnostic unless an owning stock
  contract fixes them.

## What this does not prove

This Story does not prove catalog refresh, cold restart, fork, compaction,
hosted tools, ChatGPT authentication, Mini routing, or compatibility of every
future Provider.

## Proof plan

### Preconditions

- Exact candidate artifact, one supported Grok Provider profile, one disposable
  isolated Home with only the accepted Grok credential.
- The Live profile does not set `agents.default_subagent_model`.
- The parent Thread reports its Grok Provider binding before child creation.
- The parent Turn requests logical Ultra reasoning. Deterministic contracts
  prove Ultra request mapping, immutable parent authority, child inheritance,
  and the stock child-agent seam.

### Proof-run invocation budget

One natural user task. Bound the proof run, not the product's valid internal
orchestration.

### Secret-safe evidence

Record Story ID, source and artifact identity, expected and observed
Provider/model classes, semantic child-result and parent-result
classifications, parent-link and delivery booleans, terminal classes, and
runner submission count. Correlation identifiers compared in memory; evidence
retains only equality booleans or digests. Negative evidence: no other
Provider, no runner replay, no prompts, responses, credentials, raw events, or
Thread IDs.

## Stock compatibility control

A deterministic stock contract must prove the declared upstream child-agent
seam without ChatGPT live authentication. Grok must inherit the already
resolved parent authority; it must not add a lookup, fallback, or
compatibility path to the stock child flow.

## Executable contract

`TestGrokCollaboration` in the repository-owned `grok/live` Go module. Normal
Grok delivery runs `go test ./... -count=1 -timeout 30m -run '^TestGrok'` from
`grok/live` against the exact current-run Linux archive. `llm-go/codexsdk` is
the SDK dependency, not the acceptance owner.
