# Grok hosted `x_search` date window Turn and replay

## User story

As a Grok user, I complete one Turn on the shipped profile with a Live
`[model_providers.grok.x_search]` `from_date` / `to_date` overlay so hosted
`x_search` advertises that window, then continue on the same Thread so the
replayed hosted `custom_tool_call` is accepted.

## Real path

```text
exact Grok release artifact
  -> isolated Home with shipped grok/dist/config.toml.example
     plus [model_providers.grok.x_search] from_date / to_date overlay
  -> one Grok-bound Thread with shell disabled
  -> Turn 1 /responses advertises x_search from_date and to_date
  -> Turn 1 that must use hosted x_search for recent @xai content
  -> persisted completed hosted custom_tool_call (one of the four x_search names)
  -> Turn 2 on the same Thread
  -> last /responses request replays that custom_tool_call and is accepted (2xx)
  -> both Turns reach terminal completed
```

This Story owns a real hosted `x_search` Turn whose advertised tool carries
the overlay `from_date` / `to_date`, and its history replay, on the packaged
Grok artifact. It does not replace the default unwindowed hosted `x_search`
Story and does not complete an `excluded_domains` Story.

## Acceptance

**Given** the preconditions, **when** the acceptance runner submits one Grok
App Server Turn that cannot be answered from memory and names X search, then
one follow-up Turn on the same Thread, **then**:

- Turn 1 reaches terminal `completed` with an agent message;
- every agent message that streamed a delta streamed all of its text: the
  `item/agentMessage/delta` notifications concatenate to the completed item's
  text (Grok keeps the message open across the hosted call and resumes it; no
  delta is dropped);
- Turn 1 (or the last `/responses` for that Turn) advertises a `tools` entry
  `x_search` with the overlay `from_date` and `to_date`;
- the persisted session contains a completed hosted `custom_tool_call` whose
  name is one of `x_keyword_search`, `x_semantic_search`, `x_user_search`, or
  `x_thread_fetch`;
- no client-side dispatch (`dynamic_tool_call`, error
  `custom_tool_call_output` / `function_call_output`) explains that call;
- Turn 2 on the same Thread reaches terminal `completed`;
- the last `/responses` request replays that `custom_tool_call` and Grok
  accepts it (2xx); and
- the runner submits each semantic Turn once. A Turn that completes without a
  hosted X search is a failed invocation, not a retry.

## Partial success is not completion

- `thread_bound_to_grok`: the Thread is not bound to the Grok Provider.
- `turn_completed`: Turn 1 does not reach terminal `completed` with an agent
  message.
- `streamed_text_matches_completed`: an agent message streamed part of its
  text and completed with more; the client lost deltas.
- `x_search_window_advertised`: Turn 1's last `/responses` request does not
  advertise an `x_search` tool with the overlay `from_date` and `to_date`.
- `hosted_x_search_call_recorded`: session JSONL has no completed hosted
  `custom_tool_call` named one of the four x_search tools.
- `client_dispatch_absent`: a `custom_tool_call_output` /
  `function_call_output` for that `call_id`, or a `dynamic_tool_call`-style
  item, explains the call.
- `follow_up_turn_completed`: Turn 2 does not reach terminal `completed` on
  the same Thread.
- `hosted_call_replayed_accepted`: the last `/responses` request does not
  replay that `custom_tool_call`, or Grok rejects it (non-2xx).

## Material failure boundaries

- The runner does not resubmit either Turn, including when Turn 1 completes
  without a hosted call.
- No fallback to a client tool, shell, or another Provider can complete this
  Story.
- The shipped `config.toml.example` is not modified; the date window is a
  Live overlay only.

## What this does not prove

This Story does not prove `web_search.filters.excluded_domains`, the default
unwindowed hosted `x_search` Story (`TestGrokHostedXSearch`), hosted
`web_search`, same-Thread resume after process restart, fork, compaction, or
every Grok model.

It does not claim whether `from_date` or `to_date` is inclusive or exclusive.

It does not fix the order in which the client sees the hosted call. Grok
interleaves it with the open message; the Grok ingress delivers items in
`output_index` order (Grok's own `response.output` order), so the
`custom_tool_call` reaches the client after the message it interrupted.

## Proof plan

### Preconditions

- Native Grok x_search date-window advertisement tests passed for that source
  (`codex-rs/codex-api/src/grok_request_tests.rs`).
- `TestFactXSearchDateWindow` records `accepted`.
- Isolated `CODEX_HOME` installs the shipped profile
  `grok/dist/config.toml.example` and overlays
  `[model_providers.grok.x_search]` `from_date` / `to_date`. `shell_tool` is
  disabled for the proof Turns.

### Proof-run invocation budget

One proof run contains Turn 1 and Turn 2 on one Thread. Each required Turn is
started once. A Turn that completes without a hosted `x_search` call fails
at `hosted_x_search_call_recorded`; the runner does not retry.

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
