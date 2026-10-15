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

The callable C6 runner returns bounded safe observation metadata only. It never
returns prompts, responses, credentials, raw traffic, Thread IDs or child result
values. Deterministic failure cases verify this boundary. It does not introduce
a raw-session retention sink or claim C8 artifact evidence delivery.


## Stock compatibility control

Deterministic stock tests at the same App Server and request boundaries must
pass for the frozen source. This Story does not perform a ChatGPT live Turn.

## Executable contract

`ShippedStartup` in `grok/live/shipped.go` installs the unchanged embedded source
profile and catalog in an isolated Home, verifies the complete Model DTO list,
then submits one ordinary turn and requires correlated public completion and
durable history. `ShippedPinned` selects the catalog's previous 4.6 route and
uses the retained two-turn tool/encrypted-history scenario.

C6 proves copied source asset consumption, native model/list and the deterministic
scenario oracle. Its controlled lifecycle fixtures disable tools only in test
copies. C7 owns full shipped tool/media/history HTTP composition; #339 owns
packaging and #331 owns actual backend execution. This Story remains unproven
by C6 alone. No shipped capability is disabled to make a turn pass.
