# Grok Provider-bound Thread fork

## User story

As a Grok user, I can fork an existing Grok-bound Thread and continue the
source and fork independently without changing or guessing their Provider
ownership.

## Real path

```text
isolated Home
  -> one completed Grok-bound Turn
  -> fork the Thread
  -> source Thread completes one ordinary follow-up
  -> fork completes one ordinary follow-up
  -> both remain Grok-bound
```

## Acceptance

**Given** one isolated Home and one Grok-bound Thread with a completed
ordinary Turn, **when** the App forks the Thread and sends one ordinary
follow-up to the source and one to the fork, **then**:

- the source and fork report the original Grok Provider and model;
- each follow-up completes through the same Grok Provider;
- both durable histories retain the seed Turn as an exact unchanged prefix;
- the two branches continue independently; and
- no cross-Provider fallback completes either branch.

In a deterministic isolated test Home, fail the fork follow-up at the bound
Grok Provider. The failure must remain on the fork. A later source follow-up
must complete through Grok and no OpenAI request may occur.

## Partial success is not completion

- The fork reports Grok ownership, but only one branch continues.
- Both branches complete, but either branch changes Provider or model.
- The fork loses or rewrites the seed history.
- A failed fork follow-up prevents an independent source continuation.
- A branch reaches OpenAI or completes through another Provider.

## Material failure boundaries

- The runner does not repeat a Turn.
- Cross-Provider fallback is a hard failure.

## What this does not prove

This Story does not prove cold restart, compaction, child-agent inheritance,
hosted tools, ChatGPT authentication, Mini routing, or compatibility of every
future Provider.

## Proof plan

### Preconditions

- The exact source builds the App Server used by the gate.
- The shipped Grok Provider profile shape (`wire_api = "grok_responses"`,
  `requires_openai_auth = false`, `supports_websockets = false`) is written
  into a disposable Home.
- A mock Grok gateway stands in for the remote backend.

### Proof-run invocation budget

One semantic sequence: one seed Turn, one fork, and one follow-up on each
branch. No Live credential is required.

### Secret-safe evidence

Record Story ID, source identity, lineage class, Provider class for source and
fork, durable-prefix preservation, terminal result classes, and runner
submissions. Negative evidence: no prompts, responses, credentials, Thread
IDs, or durable history content.

## Stock compatibility control

A deterministic stock contract must prove that a single-provider OpenAI Thread
with an unlisted model can fork and continue both branches without acquiring a
Grok Provider Profile, catalog, or model restrictions.

## Executable contract

`grok_fork_keeps_provider_binding_and_isolates_branch_failure` in
`codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs`. Native
`cargo test` in `grok-checks`.
