# Grok Provider-bound compaction continuation

## User story

As a Grok user, I can compact an existing Grok-bound Thread and continue it
without changing its Provider ownership or losing the history needed by the
App.

## Real path

```text
isolated Home
  -> one completed Grok-bound Turn
  -> manual compaction completes
  -> one ordinary follow-up completes
  -> the Thread remains Grok-bound
```

## Acceptance

**Given** one isolated Home and one Grok-bound Thread with a completed
ordinary Turn, **when** the App completes one manual compaction and sends one
ordinary follow-up, **then**:

- the Thread reports the original Grok Provider and model after compaction;
- the compaction item starts and completes before the follow-up completes;
- the App-visible history retains the required compacted context and the
  follow-up in the accepted order;
- the follow-up completes through the same Grok Provider; and
- no OpenAI request or cross-Provider fallback completes the scenario.

Deterministic isolated tests must also prove that compaction uses the bound
Grok Provider/model/auth projection, preserves required durable items in
order, and cannot silently continue through OpenAI.

## Partial success is not completion

- Compaction completes, but the follow-up does not complete.
- The follow-up completes through a different Provider or model.
- The App-visible compacted context or follow-up order is invalid.
- A request is sent to OpenAI or another Provider completes the continuation.
- Evidence exists only for internal history and not for the released App
  operation.

## Material failure boundaries

- The runner does not repeat compaction or the follow-up.
- Cross-Provider fallback is a hard failure.

## What this does not prove

This Story does not prove cold restart, fork, child-agent inheritance, hosted
tools, ChatGPT authentication, automatic compaction, Mini routing, or
compatibility of every future Provider.

## Proof plan

### Preconditions

- The exact source builds the App Server used by the gate.
- The shipped Grok Provider profile shape is written into a disposable Home.
- A mock Grok gateway stands in for the remote backend.

### Proof-run invocation budget

One seed Turn, one manual compaction, and one follow-up Turn. No Live
credential is required.

### Secret-safe evidence

Record Story ID, source identity, Provider class before and after compaction,
model-binding preservation, compaction and follow-up terminal classes,
history/order preservation, and runner submissions. Negative evidence: no
prompts, summaries, responses, credentials, Thread IDs, or durable history
content.

## Stock compatibility control

A deterministic stock contract must prove the upstream local-compaction item
lifecycle and post-compaction history layout.

## Executable contract

`grok_manual_compaction_keeps_provider_binding` in
`codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs`. Native
`cargo test` in `grok-checks`.

## Last proven

`grok-checks` on the `rust-v0.153.4` line
([Harness-X-Harness/codex#107](https://github.com/Harness-X-Harness/codex/pull/107)).
