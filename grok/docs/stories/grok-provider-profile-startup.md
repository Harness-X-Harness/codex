# Grok artifact starts with the supported Grok Provider profile

## User story

As a Grok user, I can start an exact release artifact with the shipped Grok
Provider profile, see the product default `grok-4.7`, and complete one
ordinary Grok Turn.

## Real path

```text
exact Grok release artifact
  -> isolated Codex Home with the shipped Grok profile grok/dist/config.toml.example
  -> stock App Server startup and model/list
  -> one Grok-bound Thread and ordinary Turn
  -> terminal completed Turn with an agent message
```

The profile uses the current config authority only: `model_provider`, raw model
ID, `model_catalog_json`, endpoint and authentication fields, and
`wire_api = "grok_responses"`.

## Acceptance

**Given** an isolated Home containing the shipped profile, **when** the user
starts the exact artifact, reads `model/list`, starts a Thread on the profile
model, and sends one ordinary input, **then**:

- App Server starts through the stock protocol;
- `model/list` returns the shipped catalog, including the profile default
  `grok-4.7` stock `Model` DTO with its current Ultra and Multi-Agent V2
  metadata;
- the Thread reports Provider `grok` and the profile model `grok-4.7`;
- the Turn reaches terminal `completed` with an agent message; and
- the acceptance runner submits the semantic Turn once.

## Partial success is not completion

- The binary starts but the bundled model is missing or altered.
- The catalog is visible but the Thread is not bound to Grok.
- The Thread starts but the ordinary Turn does not complete.
- A later repeated Turn is accepted after the first attempt failed.

## Material failure boundaries

- The runner does not resubmit a failed or missing Turn.
- No fallback to another Provider completes this Story.

## What this does not prove

This Story does not advertise an unavailable capability, prove every model
behavior, validate a future Provider profile, or prove a production/default
full-history Multi-Agent V2 child path. It does not prove Mini authorization,
routing, or accounting.

## Proof plan

### Preconditions

- Native Grok and stock Cargo tests passed for that source.
- The artifact contains the current profile and release-bundled Grok catalog.
- The credential enters only as the `GROK_API_KEY` environment variable through
  the profile's `env_key`. It is not written to evidence.

### Proof-run invocation budget

One positive semantic Grok Turn. A missing or failed terminal state is failure.

### Secret-safe evidence

GREEN records nothing. A RED run preserves, for 7 days, the redacted session
JSONL and the key-path shape of the rejected requests with the backend status
and redacted error text; no prompt text, model output, credential, or Thread ID
is recorded.


## Stock compatibility control

Deterministic stock tests at the same App Server and request boundaries must
pass for the frozen source. This Story does not perform a ChatGPT live Turn.

## Executable contract

`TestGrokBasic` in `grok/live`. The harness installs
`grok/dist/config.toml.example` as the isolated Home profile and copies
`grok/dist/models.json` beside it. `TestGrokPinnedPreviousModel` runs one
ordinary Turn on the other shipped slug. Scheduling is
[`release.md`](../release.md).
