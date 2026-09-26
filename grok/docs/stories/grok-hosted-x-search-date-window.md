# Grok hosted `x_search` date window

## User story

As a Grok user, one Turn on the shipped profile with a Live
`[model_providers.grok.x_search]` `from_date` / `to_date` overlay sends a
`/responses` request that advertises that window, and Grok accepts the request.

## Real path

```text
exact Grok release artifact
  -> isolated Home with shipped grok/dist/config.toml.example
     plus [model_providers.grok.x_search] from_date / to_date overlay
  -> one Turn
  -> last /responses advertises x_search from_date and to_date and is 2xx
```

This Story owns the date window on the packaged artifact. The unwindowed
hosted `x_search` Turn and its replay stay on `TestGrokHostedXSearch`.

## Acceptance

**Given** the preconditions, **when** the acceptance runner submits one Turn,
**then**:

- the last `/responses` request is accepted (HTTP 2xx); and
- that request advertises a `tools` entry `x_search` with the overlay
  `from_date` and `to_date`.

## Partial success is not completion

- `responses_accepted`: no `/responses` exchange was recorded, or the last one
  is not HTTP 2xx.
- `x_search_window_advertised`: that request does not advertise an `x_search`
  tool with the overlay `from_date` and `to_date`.

## Material failure boundaries

- The runner does not resubmit the Turn.
- The shipped `config.toml.example` is not modified; the date window is a Live
  overlay only.

## What this does not prove

This Story does not prove a hosted `x_search` call, history replay, client
dispatch, streamed text, the unwindowed hosted `x_search` Story, hosted
`web_search`, or every Grok model. Those hosted-call claims stay on
`TestGrokHostedXSearch`.

It does not claim whether `from_date` or `to_date` is inclusive or exclusive.

## Proof plan

### Preconditions

- Native Grok x_search date-window advertisement tests passed for that source
  (`codex-rs/codex-api/src/grok_request_tests.rs`).
- `TestFactXSearchDateWindow` records `accepted`.
- Isolated `CODEX_HOME` installs the shipped profile
  `grok/dist/config.toml.example` and overlays
  `[model_providers.grok.x_search]` `from_date` / `to_date`.

### Proof-run invocation budget

One proof run starts one Turn once.

### Secret-safe evidence

GREEN records nothing. A RED run preserves, for 7 days, the redacted session
JSONL and the key-path shape of the rejected requests with the backend status
and redacted error text; no prompt text, model output, credential, or Thread ID
is recorded.

## Stock compatibility control

A ChatGPT-bound Thread keeps the upstream Tool Plan. Deterministic stock tests
at the same request boundary (`grok_request_tests.rs`) must pass for the frozen
source. This Story does not perform a ChatGPT live Turn.

## Executable contract

`TestGrokHostedXSearchDateWindow` in `grok/live`. Requires `GROK_LIVE=1`,
`GROK_LIVE_CODEX_BIN`, and `GROK_API_KEY`. The harness installs
`grok/dist/config.toml.example` as the isolated Home profile and overlays
`[model_providers.grok.x_search]` `from_date` / `to_date`. Scheduling is
[`release.md`](../release.md).
