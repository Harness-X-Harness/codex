# Delegate an order check and receive its invoice within Grok

## User story

As a Grok user, I can ask a subagent to check a small order and receive the
checked invoice in the parent conversation, without leaving the Grok Provider
boundary.

## Real path and acceptance

One shipped Grok-bound parent receives one ordinary delegation task: check an
order of three notebooks at 4.50 and two pens at 1.25, with no tax or shipping,
and return one importable CSV invoice. The requested columns are `item`,
`quantity`, `unit_price`, and `line_total`; a total row has empty quantity and
unit price. The correct line totals are 13.50 and 2.50, and the total is 16.00.

Acceptance requires all of the following:

- A real child is durably bound to this parent and the Grok Provider, using an
  allowed shipped Grok model.
- The child completes its own successful turn with the correct complete invoice.
- That child's invoice actually enters the current parent turn through the
  stock collaboration input path.
- A subsequent durable parent terminal answer contains the correct complete
  invoice, with its message identity witnessed after that delivery.
- The owned process/callback cleanup completes, and no admitted contradiction
  invalidates the credited result.

Column and row ordering, quoting, CRLF, equivalent decimal representations, and
one CSV code block with surrounding prose are presentation choices. Missing,
duplicate or extra rows/columns, wrong quantities or money, multiple reports,
and a mere promise that the invoice is ready do not complete the task.

## Partial success is not completion

A correct parent-only invoice, a created child, a completed child saying
"ready", an undelivered child invoice, or a parent answer first seen before
child delivery does not complete this Story. A later replay of that same parent
message identity cannot become new delivery evidence. `ChildBound` and
`ChildCompleted` remain truthful partial facts after an ordinary invoice
failure; observed terminal contradictions can invalidate those facts.

Missing evidence means NOT_PROVEN, not proof that the model never tried to
spawn. Stock emits Started only after successful creation. Discovery can use
started or completed public child activity, typed collaboration receiver IDs,
or an already-observed child terminal hint, but each still requires durable
binding. A child visible only after all available discovery hints were read can
remain unproven; the observer does not invent an attempt or a cause.

## Proof plan and scope

The runner submits one parent task once, with the shipped defaults. There is no
setup turn, model-facing nonce/UUID, forced JSON schema, explicit fork mode,
child name, special reasoning effort, or fixture file write. The runner does
not resubmit a failed or incomplete task. Product-internal orchestration and
transport attempts are not additional runner invocations.

The observer uses durable official text for invoice correctness. Raw callback
text may still contain markup stripped by stock and is not an alternate answer
oracle. The bounded consumed-input observation establishes sender/recipient
and delivery; raw parent message IDs establish first-seen timing. Sufficient
post-delivery message fragments are assembled in durable creation order, not
callback completion order. Reads are not callback fences: already-generated
missing fragments may arrive within the original deadline, without another
turn or RPC. A completed child turn need not have an observed Started event or
complete raw history of every input.

This proves actual observable delegation and result participation, not hidden
model computation, dependence on an otherwise unknowable value, full-fork
history recall, or every possible future model. Exact artifact, source,
profile, backend, environment and time delimit each Live result. An unchanged
rerun is a new observation; it does not replace a failed earlier run.

### Diagnostic evidence is not an acceptance assertion

Bounded optional diagnostics preserve stage, configuration declarations,
candidate binding/read outcomes, own-turn status, invoice checks, delivery
checks, terminal errors and cleanup state. Declared/effective configuration is
not a captured provider request. Missing, dropped, unwritable or timed-out
optional logging must not change the product proof. Required runtime witnesses
are separate in-memory proof data, not evidence of logger health. No credential,
opaque reasoning, arbitrary provider traffic or secret-bearing environment is
needed. The existing sanitizer and asynchronous capture limits own retention.

## Deterministic owners

- [`shipped_test.go`](../../live/shipped_test.go),
  [`child_invoice_test.go`](../../live/child_invoice_test.go), and
  [`child_parent_reply_test.go`](../../live/child_parent_reply_test.go) own
  invoice semantics, authentic delivery, equivalent representations, stale
  message replay, missing evidence, terminal contradictions and partial facts.
- `TestShippedChildFixture/delegated_invoice`,
  `/parent_only_invoice_rejected`, and `/child_without_invoice_rejected` in
  [`child_test.go`](../../tests/integration/child_test.go) run the actual Codex
  binary against controlled Grok HTTP, including child execution, real result
  delivery and parent continuation. These are not real model Live evidence.
- Retained `inherited_spawn_history`, `delivered_result`, `output_schema_scope`
  and `inherited_result_structured_negative` subtests are **synthetic direct
  HTTP-handler fixtures**, not native child-composition acceptance.
- Stock `spawn_numeric_tests::fork_turns_preserves_defaults_and_string_modes`,
  `agent/control_tests::spawn_agent_can_fork_parent_thread_history_with_sanitized_items`
  and `spawn_agent_fork_from_paginated_parent_uses_model_context_prefix` own
  fork semantics. App Server [provider binding](../../../codex-rs/app-server/tests/suite/v2/grok_provider_binding.rs)
  and [public child listener](../../../codex-rs/app-server/tests/suite/v2/grok_child_public_listener.rs)
  own inherited authority and listener delivery. Per-turn output-schema tests
  own that separate protocol constraint; they are not child invoice proof.

## Contract revision

The user-approved 2026-10-30 product-first revision replaces the former #387
Live marker/UUID freshness and full-fork recall probe with this ordinary
business outcome. The removed Live assertions remain distinct deterministic
compatibility concerns where named above; no previous failed Live result is
relabelled as a pass. Only a new identified package/backend run can prove this
revised Story.
