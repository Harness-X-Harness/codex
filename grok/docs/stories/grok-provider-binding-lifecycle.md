# Grok child agent inherits parent Provider authority

## User story

As a Grok user, I can delegate one bounded task from a Grok-bound parent
Thread to a child agent, receive the child's result, and complete the parent
task without leaving the Grok Provider boundary.

## Real path

```text
one observably Grok-bound parent Thread
  -> one completed bounded setup Turn with a fresh random setup marker
  -> one original natural user-level delegation task
  -> a runtime-created default/full-history child completes a bounded task
  -> the child result returns to the parent
  -> the parent uses that result and completes
```

## Acceptance

**Given** one parent Thread that visibly reports its Grok Provider binding,
**when** the user submits one natural task that requires bounded delegation,
**then**:

- at least one runtime-created default/full-history child is visible under the
  parent and reports the expected Grok Provider;
- the deterministic stock inheritance contract establishes that an
  unoverridden natural spawn uses the expected default/full-history child model
  class;
- the child correctly recalls the setup marker from inherited parent context;
  the runner does not include the marker value in the delegation task, and no
  later child input or alternate tool lookup may supply it;
- a delegated child produces a fresh, safe bounded result that was not known to
  the parent when the user task was submitted;
- that result is delivered to the parent through the stock collaboration
  lifecycle;
- the parent reaches a visible terminal result that contains the same fresh
  child result; and
- the accepted child and parent result cannot be supplied by another Provider
  or by an acceptance-runner replay.

## Partial success is not completion

- The parent completes without consuming a valid delegated result.
- A result from another Provider or a second lookup path is accepted as the
  Grok child result.
- Only an internal child constructor or inheritance fixture is exercised.

## Material failure boundaries

- The runner submits one natural user task and does not resubmit it.
- No fallback to another Provider completes this Story.
- Child count, collaboration-tool calls, Provider responses, wait lifecycle
  events, ordering, and duration are diagnostic unless an owning stock
  contract fixes them.

## What this does not prove

This Story does not prove catalog refresh, cold restart, fork, compaction,
hosted tools, ChatGPT authentication, Mini routing, or compatibility of every
future Provider.

## Proof plan

### Preconditions

- Exact candidate artifact, one supported Grok Provider profile, one disposable
  isolated Home with only the accepted Grok credential.
- The Live profile does not set `agents.default_subagent_model`.
- The parent Thread reports its Grok Provider binding before child creation.
- The setup Turn keeps profile defaults; the delegation Turn requests logical
  Ultra reasoning. Deterministic contracts
  prove Ultra request mapping, immutable parent authority, child inheritance,
  and the stock child-agent seam.

### Proof-run invocation budget

One bounded seed setup Turn plus one original natural delegation task, each
submitted once. The setup supplies a fresh 128-bit random marker only to the
parent's setup context. The original delegation task asks the child to recall
that marker and produce a fresh UUID; it never contains the marker value. This
proves observable use of inherited context without requiring stock's paginated
child UI history to repeat the parent's records. It is not a retry of the
delegation task. No extra child task is submitted by the runner. Bound the proof
run, not the product's valid internal orchestration.

### Secret-safe evidence

The callable runner returns bounded safe observation metadata only. It never
returns prompts, responses, credentials, raw traffic, Thread IDs or child result
values. Deterministic failure cases verify this boundary. It does not introduce
a raw-session retention sink or establish package provenance.


## Stock compatibility control

A deterministic stock contract must prove the declared upstream child-agent
seam without ChatGPT live authentication. Grok must inherit the already
resolved parent authority; it must not add a lookup, fallback, or
compatibility path to the stock child flow.

## Executable contract

`ShippedChildCollaboration` in `grok/live/shipped_child.go` consumes unchanged
shipped assets and records one completed setup Turn before the single natural
delegation task. The parent's setup must retain its successful turn identity and
ordered user/assistant content; reconstructed item IDs and diagnostic metadata
are not part of that conversation contract. A qualifying child must report the setup marker in its completed result, with
complete observed own-input coverage proving that the marker was not supplied
again before that result. Auxiliary tool activity is not itself disqualifying;
lineage alone is still insufficient.

Stock paginated subagent history intentionally omits inherited model records
from the child's public turns. Exact public child seed item identities, content
and order are therefore not a Live requirement. The actual App Server-to-Grok
controlled HTTP prerequisite independently verifies the complete inherited seed
content and order on the wire with unchanged shipped capabilities. The child
request must contain the delegated task, but inherited tool records, normalized
output wording and task-envelope presentation are not a unique required shape.
That controlled prerequisite does not replace real child recall and parent result
consumption.

The runner accepts a bounded fresh UUID from a correctly bound, completed child
turn when the parent's terminal reply contains it. UUID casing, additional UUIDs
and surrounding prose are not exact-output constraints. Parent prose, additional
orchestration and incidental failed/running children or spawns do not supply
credit or invalidate an otherwise qualifying result. It can recognize a causal
result before a child's later follow-up. Missing or incorrect recalled markers,
markers supplied through later child inputs, inherited
results, mismatched Provider/model/ownership and result values already in parent
text before that child's Started activity or supplied to the child are rejected.
This includes the current delegation turn: a full fork can inherit a parent's
pre-spawn final answer even when paginated public child history hides it.
Ordinary hexadecimal letter casing does not make a supplied value fresh. This is
a bounded proof over the observed plaintext inputs and results; it does not
establish noninterference for arbitrary encodings or covert channels. Counts
remain bounded observation budgets; neither setup nor delegation is resubmitted.
The UUID is this probe's provenance witness, not a general ban on reusing parent
results. A child provider request does not establish new internal model computation.

The observer discovers children through actual public V2 `subAgentActivity`
started items and binds their IDs/paths to public child lineage/history. The
parent's existing `experimentalRawEvents` opt-in is inherited by child listeners.
Ordered `rawResponseItem/completed` evidence must contain the child's consumed
`AgentMessage` inputs and the same marker and nonce as its completed public
result, without requiring identical surrounding prose. Every own turn through
that result needs complete observed input/lifecycle coverage. Any pre-result supplied nonce prevents credit; later mentions, later
unrelated turns and closure after a qualified completion do not. Observed terminal
conflicts invalidate the exact credited child/turn, not independent candidates.
Earlier own turns may fail if their observed terminal status and error presence
agree with durable history; diagnostic error wording need not be identical. Only
the credited result must complete successfully. Inherited user
input and final assistant messages are checked for supplied values; public
diagnostics are not treated as inherited model input.
The setup transcript's user and assistant text is independently parent-known,
including commentary and phase-empty replies, while item IDs/metadata are not.
Missing/closed/partial streams or opaque encrypted input before the result remain
NOT_PROVEN, without a retry or failure attribution. Raw content stays in memory
within 512 event / 1 MiB bounds and is never emitted as evidence.
Because history reads and listener delivery are independent, missing generated
notifications may still arrive within the original deadline. The runner waits
only for that evidence, within the same event bound, and makes no additional RPC
or semantic invocation. Known contradictions settle without a deadline wait.

[Rust provider-binding tests](../../../codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs)
and the [public child listener](../../../codex-rs/app-server/tests/suite/v2/grok_child_public_listener.rs)
own Provider inheritance and stock listener delivery. [Offline Go scripts](../../live/shipped_test.go)
validate the retained scenario oracle. [`TestShippedChildFixture`](../../tests/integration/child_test.go)
executes real Codex against controlled Grok HTTP with unchanged shipped
capabilities, checks inherited seed content/order and fresh child result plus
parent consumption, and rejects a parent-supplied result. This is implemented
composition coverage, not evidence of real model delegation.
[`TestLive/shipped-child`](../../tests/live/live_test.go) requires its own
identified package/backend run before this complete shipped Story is proven.
