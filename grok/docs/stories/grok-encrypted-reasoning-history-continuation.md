# Encrypted reasoning survives same-thread continuation

## User story and real path

As a Grok user, I can continue the same thread after a reasoning/tool turn and
recover its fresh tool result without submitting that result again.

Exact binary -> configured fixture -> one thread -> reasoning/tool turn N ->
canonical history -> turn N+1 -> visible terminal result. Exercise the primary
and pinned model fixtures separately; neither defines shipped catalog policy.

## Acceptance

Given an exact binary with Provider, encrypted-history and relevant #322 tool
proof, an isolated fixture and a usable default execution environment, when turn
N calls the named dynamic tool and receives a freshly generated result absent
from its prompt, then N completes with that result in its final assistant reply.
When N+1 is submitted once on the same thread without the runner supplying that
result, then N+1 completes with the same result in its final reply.

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
- Keep identity correlation and fresh tool content only in memory. Retain safe
  subject metadata, completion/binding/recall booleans, item/invocation counts
  and last proven stage; no raw items, opaque values, private text or stable IDs.
- Missing required evidence means NOT_PROVEN. Failure attribution is separate:
  repository, external_environment, or inconclusive only as evidence supports.

This does not prove compaction, fork, multi-agent behavior, every tool or model,
shipped profiles, future backend availability, or present product acceptance.
Required CI tests the harness deterministically with backend opt-ins off.
