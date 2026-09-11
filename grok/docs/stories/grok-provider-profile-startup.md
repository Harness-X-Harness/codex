# Grok artifact starts with the supported Grok Provider profile

## User story

As a Grok user, I can start an exact release artifact with the shipped Grok
Provider profile, see the release-bundled `grok-4.6` model, and complete one
ordinary Grok Turn.

## Real path

```text
exact Grok release artifact
  -> isolated Codex Home with the shipped Grok profile
  -> stock App Server startup and model/list
  -> one Grok-bound Thread and ordinary Turn
  -> terminal completed Turn with an agent message
```

The profile uses the current config authority only: `model_provider`, raw model
ID, endpoint and authentication fields, and `wire_api = "grok_responses"`.

## Acceptance

**Given** an isolated Home containing the shipped profile, **when** the user
starts the exact artifact, reads `model/list`, starts a `grok-4.6` Thread, and
sends one ordinary input, **then**:

- App Server starts through the stock protocol;
- `model/list` returns the release-bundled `grok-4.6` stock `Model` DTO with
  its current Ultra and Multi-Agent V2 metadata;
- the Thread reports Provider `grok` and model `grok-4.6`;
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

- Artifact source SHA, archive identity, and checksum are known.
- Native Grok and stock Cargo tests passed for that source.
- The artifact contains the current profile and release-bundled Grok catalog.
- One usable Grok credential is available without being written to evidence.

### Proof-run invocation budget

One positive semantic Grok Turn. A missing or failed terminal state is failure.

### Secret-safe evidence

Record Story ID, source SHA, validation run, archive identity and checksum,
profile identity, model and Provider labels, terminal state, and runner
submission count. Negative evidence: no runner re-invocation, no other
Provider, no credentials, prompts, responses, raw traffic, URLs, account
identifiers, or Thread and Session IDs.

## Stock compatibility control

Deterministic stock tests at the same App Server and request boundaries must
pass for the frozen source. This Story does not perform a ChatGPT live Turn.

## Executable contract

`TestGrokBasic` in the repository-owned `grok/live` Go module. Normal Grok
delivery runs `go test ./... -count=1 -timeout 30m -run '^TestGrok'` from
`grok/live` against the exact current-run Linux archive. `llm-go/codexsdk` is
the SDK dependency, not the acceptance owner. The GREEN run is the evidence.

## Last proven

Grok workflow run
[34549988338](https://github.com/Harness-X-Harness/codex/actions/runs/34549988338)
on 2026-09-11 for published source `6a22ea48d22f21c0dc06894b107bfc918054870b`.
