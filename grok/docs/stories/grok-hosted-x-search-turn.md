# Grok hosted `x_search` Turn and follow-up replay

## User story

As a Grok user, I complete one Turn that requires current X content through
the packaged Grok App Server so the backend-hosted `x_search` call is recorded
instead of dispatched as a client tool, and the next Turn on the same Thread
replays that `custom_tool_call` without a backend rejection.

## Real path

```text
exact Grok release artifact
  -> isolated Home and workspace with shell disabled
  -> one Grok-bound Thread
  -> Turn 1 whose prompt requires current X content
  -> completed hosted custom_tool_call (one of the four x_search names)
  -> Turn 1 reaches terminal completed with an agent message
  -> Turn 2 on the same Thread
  -> outbound /responses replays the custom_tool_call and Grok accepts it
```

This Story owns the shipped Grok-native `x_search` path: dialect advertisement,
ingress recognition of a completed hosted `custom_tool_call`, durable recording
without client dispatch, and follow-up replay. It does not complete a Mini
transport Story or an `x_search` date-window Story.

## Acceptance

**Given** the preconditions, **when** the acceptance runner submits one Grok
App Server Turn that cannot be answered from memory and names X search, then
one follow-up Turn on the same Thread, **then**:

- Turn 1 reaches terminal `completed` with an agent message;
- every agent message that streamed a delta streamed all of its text: the
  `item/agentMessage/delta` notifications concatenate to the completed item's
  text (Grok keeps the message open across the hosted call and resumes it; no
  delta is dropped);
- the persisted session contains a completed hosted `custom_tool_call` whose
  name is one of `x_keyword_search`, `x_semantic_search`, `x_user_search`, or
  `x_thread_fetch`;
- no client-side dispatch (`dynamic_tool_call`, error
  `custom_tool_call_output` / `function_call_output`) explains that call;
- Turn 2 on the same Thread completes;
- the last outbound `/responses` request replays that `custom_tool_call` and
  Grok accepts it (2xx); and
- the runner submits each semantic Turn once.

## Partial success is not completion

- `thread_bound_to_grok`: Thread Provider is not Grok.
- `turn_completed`: Turn 1 is not terminal completed with an agent message.
- `streamed_text_matches_completed`: an agent message streamed part of its
  text and completed with more; the client lost deltas.
- `hosted_x_search_call_recorded`: session JSONL has no completed hosted
  `custom_tool_call` named one of the four x_search tools.
- `client_dispatch_absent`: a `custom_tool_call_output` /
  `function_call_output` for that `call_id`, or a `dynamic_tool_call`-style
  item, explains the call.
- `follow_up_turn_completed`: Turn 2 is not terminal completed on the same
  Thread.
- `hosted_call_replayed_accepted`: the last `/responses` request does not
  replay that `custom_tool_call`, or Grok rejects it (non-2xx).

## Material failure boundaries

- The runner does not resubmit either Turn, including when Turn 1 completes
  without a hosted call.
- No fallback to a client tool, shell, or another Provider can complete this
  Story.
- A Turn 2 rejection is recorded as observed backend error text; egress is
  not stripped to make the replay succeed.

## What this does not prove

This Story does not prove `x_search` `from_date` / `to_date` (B2), widening
`is_provider_hosted_tool_call` to every completed `custom_tool_call` (B2),
hosted `web_search`, structured `structured_edit`, Mini routing, every Grok model, or
every x_search sub-tool Grok may add later.

It does not fix the order in which the client sees the hosted call. Grok
interleaves it with the open message; the Grok ingress delivers items in
`output_index` order (Grok's own `response.output` order), so the
`custom_tool_call` reaches the client after the message it interrupted.

## Proof plan

### Preconditions

- Native Grok Provider predicate test for `is_provider_hosted_tool_call`
  passed for that source.
- The dialect appends `{type: x_search}` when tools are non-empty.
- Isolated `CODEX_HOME` and workspace. `shell_tool` is disabled for the
  proof Turns.

### Proof-run invocation budget

One Grok App Server Turn that requires current X content, then one follow-up
Turn on the same Thread. The runner does not resubmit after a missing hosted
call, a client dispatch, or a failed terminal result.

### Secret-safe evidence

GREEN records nothing. A RED run preserves, for 7 days, the redacted session
JSONL and the key-path shape of the rejected requests with the backend status
and redacted error text; no prompt text, model output, credential, or Thread ID
is recorded.


## Stock compatibility control

A ChatGPT-bound Thread keeps the upstream Tool Plan and the default
`is_provider_hosted_tool_call` (false). The native predicate test is Grok
Provider-only. This Story does not perform a ChatGPT live Turn.

## Executable contract

`TestGrokHostedXSearch` in `grok/live`. Requires `GROK_LIVE=1`,
`GROK_LIVE_CODEX_BIN`, and `GROK_API_KEY`. The harness installs
`grok/dist/config.toml.example` as the isolated Home profile. Scheduling is
[`release.md`](../release.md).
