# Grok Provider-bound cold restart and resume

## User story

As a Grok user, I can close and restart the App, resume an existing Grok-bound
Thread, and complete one ordinary follow-up without changing its Provider
ownership or durable history.

## Real path

```text
isolated Home
  -> one completed Grok-bound Turn
  -> App process closes
  -> new App process starts
  -> same Thread resumes
  -> one ordinary follow-up completes through Grok
```

## Acceptance

**Given** one isolated Home and one Grok-bound Thread with a completed
ordinary Turn, **when** the App process closes, a new App process starts,
resumes that Thread, and sends one ordinary follow-up, **then**:

- the resumed Thread reports the same Grok Provider and model;
- the follow-up completes through the same Grok Provider;
- the original durable Turn remains an exact unchanged prefix of the resumed
  Thread history; and
- no OpenAI Provider request or cross-Provider fallback completes the
  scenario.

In a deterministic isolated test Home, remove the configured Grok Provider
Profile after the seed Turn and restart the App. Resume must fail with
`ProviderUnavailable` before any Provider request. Restore the same stable
profile, restart again, and resume successfully without rewriting the stored
history.

## Partial success is not completion

- A hot in-process resume succeeds without closing the App.
- Resume reports Grok ownership, but the follow-up reaches another Provider.
- Resume succeeds only after rewriting or dropping the original durable Turn.
- Missing Provider Profile fails only after Provider egress.
- Restoring the Provider Profile creates a replacement Thread instead of
  resuming the stored one.

## Material failure boundaries

- The runner does not repeat either Turn.
- Cross-Provider fallback is a hard failure.

## What this does not prove

This Story does not prove fork, compaction, child-agent inheritance, hosted
tools, ChatGPT authentication, Mini routing, or compatibility of every future
Provider.

## Proof plan

### Preconditions

- The exact source builds the App Server used by the gate.
- The shipped Grok Provider profile shape is written into a disposable Home.
- A mock Grok gateway stands in for the remote backend.

### Proof-run invocation budget

One seed Turn, one App restart, one resume, and one follow-up Turn. No Live
credential is required.

### Secret-safe evidence

Record Story ID, source identity, Provider class before and after restart,
restart and resume result classes, durable-prefix preservation, terminal
status, and runner submissions. Negative evidence: no prompts, responses,
credentials, Thread IDs, or durable history content.

## Stock compatibility control

A deterministic stock contract must prove that a single-provider OpenAI Thread
with an unlisted model survives the same cold-restart and resume seam without
acquiring a Grok Provider Profile, catalog, or model restrictions.

## Executable contract

`grok_cold_restart_resume_keeps_provider_binding` in
`codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs`. Native
`cargo test` in `grok-checks`.
