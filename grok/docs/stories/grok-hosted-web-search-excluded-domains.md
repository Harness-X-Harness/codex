# Grok hosted `web_search` domain blocklist

## User story

As a Grok user, one Turn on the shipped profile with stock
`[tools.web_search] excluded_domains` sends a `/responses` request that
advertises that filter, and Grok accepts the request.

## Real path

```text
exact Grok release artifact
  -> isolated Home with shipped grok/dist/config.toml.example
     plus stock [tools.web_search] excluded_domains overlay
  -> one Turn
  -> last /responses advertises web_search.filters.excluded_domains and is 2xx
```

This Story owns the blocklist on the packaged artifact. The unfiltered hosted
`web_search` Turn and its replay stay on `TestGrokHostedWebSearch`.

## Acceptance

**Given** the preconditions, **when** the acceptance runner submits one Turn,
**then**:

- the last `/responses` request is accepted (HTTP 2xx); and
- that request advertises a `tools` entry `web_search` with
  `filters.excluded_domains` containing the configured domain.

## Partial success is not completion

- `responses_accepted`: no `/responses` exchange was recorded, or the last one
  is not HTTP 2xx.
- `excluded_domains_advertised`: that request does not advertise a `web_search`
  tool with `filters.excluded_domains` containing the configured domain.

## Material failure boundaries

- The runner does not resubmit the Turn.
- The shipped `config.toml.example` is not modified; the blocklist is a Live
  overlay only.
- The overlay is `excluded_domains` only; it is not combined with
  `allowed_domains`.

## What this does not prove

This Story does not prove a hosted `web_search_call`, history replay, streamed
text, `web_search.filters.allowed_domains`, standalone `/alpha/search`,
hosted `x_search`, or every Grok model. Those hosted-call claims stay on
`TestGrokHostedWebSearch`.

## Proof plan

### Preconditions

- Native Grok web-search advertisement tests passed for that source
  (`codex-rs/codex-api/src/grok_request_tests.rs`).
- `TestFactWebSearchExcludedDomains` records `accepted`.
- Isolated `CODEX_HOME` installs the shipped profile
  `grok/dist/config.toml.example` and overlays stock
  `[tools.web_search] excluded_domains`.

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

`TestGrokHostedWebSearchExcludedDomains` in `grok/live`. Requires `GROK_LIVE=1`,
`GROK_LIVE_CODEX_BIN`, and `GROK_API_KEY`. The harness installs
`grok/dist/config.toml.example` as the isolated Home profile and overlays
stock `[tools.web_search] excluded_domains`. Scheduling is
[`release.md`](../release.md).
