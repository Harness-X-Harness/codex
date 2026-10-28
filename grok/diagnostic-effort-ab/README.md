# Temporary same-time profile control — never merge

Dispatch-only experiment. A uses the isolated reasoning fixture; B uses the
original shipped profile/catalog with the existing controlled-endpoint helper.
Both use grok-4.6, the original first-turn history-probe prompt, the same strict
token schema and dynamic probe. Neither explicitly overrides effort. The shipped
catalog defaults to high, whereas isolated has no effort. This is an intentional
multi-variable positive control, not evidence that any one profile field causes
an outcome. Sandbox/approval settings are not elevated or changed.

Fixed binary artifact 12769806175, runtime SHA-256
47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16,
harness e3c0812c7d47c567d42a181c921a30aa3c35cdc0. Shipped's config default model
4.7 is preserved, but the actual thread and outgoing request must be 4.6.

Only A/B, at most four real HTTP requests (two per arm). No reverse pair, automatic
retries or redirects. Errors never refund budget. A second request must be the
paired tool result. Unknown failure or incomplete cleanup stops the batch.

Before the backend secret is available, actual-binary loopback checks verify each
profile, matching task/schema, expected effort, both tool rounds, cleanup and a
separate single-request 403. All request differences are reported safely, but the
control does not require unlike profiles to have identical bodies. The exact
runtime/source-bound passing proof gates backend access. Actual request model
and the hard relay budget remain enforced during the real phase.

Header audit records known incoming/forwarded/dropped header names without values;
the forwarding policy is unchanged in this batch. JSON diagnostics contain bounded
shape, types, sizes, hashes, request IDs, UTC and outcomes, never private reasoning,
ciphertext, credentials or full message content. SSE is buffered with a 90-second
socket timeout. These shared transport changes limit production extrapolation.
Do not infer upstream receipt solely from send_attempted=true.

Prior effort and prompt experiments each used two real requests and both arms
received initial 403. This separately bounded batch tests whether shipped still
succeeds concurrently through the same relay. Native-direct experimentation is
not implemented: cancelling an SDK cursor does not necessarily cancel a native
turn, so it would not by itself enforce a one-request budget.
