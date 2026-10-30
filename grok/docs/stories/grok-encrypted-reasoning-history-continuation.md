# Encrypted reasoning survives same-thread continuation

## User story and real path

As a Grok user, I can continue the same thread after a reasoning/tool turn and
use a previously looked-up apple price to calculate a new quantity without
looking up or submitting that price again.

Exact binary -> configured fixture -> one thread -> reasoning/tool turn N ->
canonical history -> turn N+1 -> visible terminal result. Exercise the primary
and pinned model fixtures separately; neither defines shipped catalog policy.
Leave turn effort unset: this Story proves reasoning and history continuity, not
acceptance of a particular effort value. All reasoning/tool/history evidence
below remains required.

## Acceptance

Given an exact binary with Provider, encrypted-history and relevant tool
proof, an isolated fixture and a usable default execution environment, when turn
N asks the price of apples per kilogram and the cost of two kilograms, calls
the price-lookup dynamic tool and receives this run's ordinary integer unit
price absent from its prompt, then N completes with the correct total. N+1 asks
for the cost of three kilograms without a new lookup. Both replies use the SDK
output schema; the natural task text does not instruct JSON or exact copying.
Both N and N+1 explicitly request the same schema on their own turn submission:

```json
{"type":"object","properties":{"amount":{"type":["number","null"]}},"required":["amount"],"additionalProperties":false}
```

The schema never embeds the price or an expected amount. Given the actual
tool price p, `amount` must be 2p on N and 3p on N+1; `null` is unavailable and
not success. This proves using the prior price, not reproducing a random marker.
N+1 is submitted once on the same thread without the runner supplying the price. Explicit `final_answer` items take priority over
commentary/partial text; without an explicit final, phase-unknown replies retain
legacy compatibility. All eligible final fragments form one bounded JSON result.
Do not select the first parseable object or accept an amount substring in prose.
Missing/duplicate/unknown fields, extra JSON values, non-numeric non-null amounts,
and trailing prose reject the format. Replayed item identities do not add
content; commentary or partial text cannot supply missing final recall.

Require actual completed reasoning, an opaque encrypted reasoning item from N,
and a completed named tool call. Native projection/composition proof establishes
unchanged encrypted replay; Live establishes real reasoning and the externally
observable history-dependent result. Rollout storage alone cannot complete the
Story or substitute for either final reply.

## Partial success and failure boundaries

One completed turn, a tool result only in storage, another thread, another
Provider, or runner resubmission does not complete this Story. Do not constrain
product-internal same-Provider retries or the number of internal tool calls when
the semantic result is the same. No automatic rerun follows NOT_PROVEN.

## Proof plan

- Bind the actual binary digest/source/target, harness revision, model fixture,
  environment and observation time; require the relevant native gates first.
- Per fixture: one process initialization, one thread creation, N once, N+1 once.
  Dynamic-tool replies are responses inside N, not new semantic turn invocations.
- Causally matching App Server final items/turn items prove visible replies.
  Experimental raw Responses items can establish the encrypted-item fact;
  equivalent causally linked evidence is acceptable when it proves that same
  fact at the product boundary. Reasoning text alone does not prove encryption.
- Keep the safe verdict summary separate from bounded task diagnostics. Retain
  actual submitted prompts/schema, public final fragments and aggregate reply,
  parse/match outcome, and the synthetic tool request/result needed to compare
  the calculated amounts. Include run linkage, lifecycle and capture completeness.
  Do not retain raw wire/history dumps, encrypted reasoning values, private
  reasoning content, credentials or unrelated environment data. Diagnostic
  capture failure does not change the business verdict; report it separately.
- Missing required evidence means NOT_PROVEN. Failure attribution is separate:
  repository, external_environment, or inconclusive only as evidence supports.

This does not prove compaction, fork, multi-agent behavior, every tool or model,
shipped profiles, future backend availability, or present product acceptance.
Required CI tests the harness deterministically with backend opt-ins off.

## Executable owners

[`TestReasoningHistoryPublicProtocol`](../../live/reasoning_history_test.go) and
[SDK observer tests](../../live/sdk_reasoning_observer_test.go) own offline
correlation and false-evidence controls. [`TestReasoningHistoryFixture`](../../tests/integration/reasoning_test.go)
executes real Codex with controlled HTTP and verifies unchanged encrypted replay,
paired price output and the continuation without price resubmission. The fixture supplies
answers and does not prove real model calculation or history use. [`TestLive/reasoning-primary` and
`reasoning-pinned`](../../tests/live/live_test.go) require identified backend runs.
