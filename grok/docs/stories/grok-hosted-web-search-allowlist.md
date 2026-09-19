# Grok hosted `web_search` domain allowlist Turn and replay

## User story

As a Grok user, I complete one Turn on the shipped profile with stock
`[tools.web_search] allowed_domains` so hosted `web_search` advertises that
filter, then continue on the same Thread so the replayed `web_search_call` is
accepted.

## Real path

```text
exact Grok release artifact
  -> isolated Home with shipped grok/dist/config.toml.example
     plus stock [tools.web_search] allowed_domains overlay
  -> one Grok-bound Thread with shell disabled
  -> Turn 1 /responses advertises web_search.filters.allowed_domains
  -> Turn 1 that must use hosted web_search for current allowlisted-site content
  -> persisted response_item type web_search_call
  -> Turn 2 on the same Thread
  -> last /responses request replays web_search_call and is accepted (2xx)
  -> both Turns reach terminal completed
```

This Story owns a real hosted `web_search` Turn whose advertised tool carries
stock `filters.allowed_domains`, and its history replay, on the packaged Grok
artifact. It does not replace the default unfiltered hosted `web_search`
Story and does not complete an `excluded_domains` Story.

## Acceptance

**Given** the preconditions, **when** the acceptance runner submits one Grok
App Server Turn that cannot be answered from memory and names the allowlisted
site, then one follow-up Turn on the same Thread, **then**:

- Turn 1 reaches terminal `completed` with an agent message;
- every agent message that streamed a delta streamed all of its text: the
  `item/agentMessage/delta` notifications concatenate to the completed item's
  text (Grok keeps the message open across the hosted calls and resumes it;
  no delta is dropped);
- Turn 1 (or the last `/responses` for that Turn) advertises a `tools` entry
  `web_search` with `filters.allowed_domains` containing the configured
  domain;
- the persisted session contains a `response_item` of type `web_search_call`;
- Turn 2 on the same Thread reaches terminal `completed`;
- the last `/responses` request replays a `web_search_call` input item and
  that request is accepted (HTTP 2xx); and
- the runner submits each semantic Turn once. A Turn that completes without a
  search is a failed invocation, not a retry.

## Partial success is not completion

- `thread_bound_to_grok`: the Thread is not bound to the Grok Provider.
- `turn_completed`: Turn 1 does not reach terminal `completed` with an agent
  message.
- `streamed_text_matches_completed`: an agent message streamed part of its
  text and completed with more; the client lost deltas.
- `allowed_domains_advertised`: Turn 1's last `/responses` request does not
  advertise a `web_search` tool with `filters.allowed_domains` containing the
  configured domain.
- `hosted_web_search_call_recorded`: the persisted session has no
  `response_item` of type `web_search_call` (a completed `custom_tool_call` or
  another item type is not this path).
- `follow_up_turn_completed`: Turn 2 does not reach terminal `completed` on
  the same Thread.
- `web_search_call_replayed_accepted`: the last `/responses` request does not
  replay a `web_search_call` input item with HTTP 2xx.

## Material failure boundaries

- The runner does not resubmit either Turn.
- No fallback to another Provider, a shell, or standalone `/alpha/search` can
  complete this Story.
- The shipped `config.toml.example` is not modified; the allowlist is a Live
  overlay only.

## What this does not prove

This Story does not prove `web_search.filters.excluded_domains`, the default
unfiltered hosted `web_search` Story, standalone `/alpha/search`
(`StandaloneWebSearch`), hosted `x_search`, same-Thread resume after process
restart, fork, compaction, or every Grok model.

It does not fix the order in which the client sees the hosted items. Grok
interleaves them with the open message; the Grok ingress delivers items in
`output_index` order (Grok's own `response.output` order), so the
`web_search_call` items reach the client after the message they interrupted.

## Proof plan

### Preconditions

- Native Grok web-search advertisement tests passed for that source
  (`codex-rs/core/tests/suite/grok_web_search.rs`).
- `TestFactWebSearchAllowedDomains` records `accepted`.
- Isolated `CODEX_HOME` installs the shipped profile
  `grok/dist/config.toml.example` and overlays stock
  `[tools.web_search] allowed_domains`. `shell_tool` is disabled for the
  proof Turns.

### Proof-run invocation budget

One proof run contains Turn 1 and Turn 2 on one Thread. Each required Turn is
started once. A Turn that completes without a hosted `web_search_call` fails
at `hosted_web_search_call_recorded`; the runner does not retry.

### Secret-safe evidence

GREEN records nothing. A RED run preserves, for 7 days, the redacted session
JSONL and the key-path shape of the rejected requests with the backend status
and redacted error text; no prompt text, model output, credential, or Thread ID
is recorded.


## Stock compatibility control

A ChatGPT-bound Thread keeps the upstream Tool Plan. Deterministic stock tests
at the same request boundary (`grok_web_search.rs`) must pass for the frozen
source. This Story does not perform a ChatGPT live Turn.

## Executable contract

`TestGrokHostedWebSearchAllowlist` in `grok/live`. Requires `GROK_LIVE=1`,
`GROK_LIVE_CODEX_BIN`, and `GROK_API_KEY`. The harness installs
`grok/dist/config.toml.example` as the isolated Home profile and overlays
stock `[tools.web_search] allowed_domains`. Scheduling is
[`release.md`](../release.md).
