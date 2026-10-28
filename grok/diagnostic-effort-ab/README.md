# Temporary effort A/B diagnostic — never merge

This dispatch-only branch is an authorized, bounded experiment, not a change to the
formal Live oracle. The registered grok-live workflow is overridden on this branch
only. Do not merge this branch or rerun its workflow.

Fixed subject: artifact 12769806175, runtime SHA-256
47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16,
harness e3c0812c7d47c567d42a181c921a30aa3c35cdc0, model grok-4.6.
A leaves effort unset; B sets high. The original first-turn task, strict token
schema, dynamic tool and isolated profile remain identical. This is not the
second-turn history recall acceptance test.

The first CI stage has no backend secret. It validates the fixed package, compiles
the driver, runs unit tests and uses the actual binary against a synthetic loopback
server. Both initial and tool-continuation requests must differ only in effort,
after explicitly normalizing fresh-session cache keys, temporary working directory
and fresh random tool token. It also checks that a 403 causes exactly one request.
Only an exact source/runtime-bound passing offline proof permits the secret step.
Local unit tests pass; the actual-binary loopback gate cannot run in the local
sandbox because its daemon directory is policy-restricted. CI must establish it.

Real order is A/B, then B/A only if the first pair differs in success. At most four
arms, two HTTP sends per arm and eight total; reservations happen before sending.
No redirects or automatic retries. A second send must be the paired tool result,
not an initial-request retry. HTTP errors lock the arm. Unknown/transport/cleanup
failures stop the experiment; they are not evidence of model refusal. No reruns.

The fixed loopback relay forwards to the existing same-upstream proxy endpoint.
It buffers SSE and has a 90-second socket timeout; these shared transport changes
limit extrapolation to production. Safe evidence contains counters, statuses,
request IDs and semantic result summaries, never credentials, raw requests,
private reasoning or ciphertext. Only explicitly listed JSON files are uploaded.
A single successful arm does not prove causality. No production assertion changes.

## Offline diagnostic completeness

Run 40609515160 verified the package and both actual-binary loopback tool rounds,
but stopped on a non-effort body difference before the real step (zero real
requests). Its old comparison did not retain the differing paths; the cause is
not yet established. The updated gate records both request pairs, safe raw and
normalized difference summaries (paths, types, sizes, hashes and structural
markers, never text contents), arm cleanup and an independent denial/retry check
before aggregating. Unknown differences still fail closed; no additional fields
are silently normalized. The real relay records per-send UTC, phase, bounded safe
request shape, status and request IDs. An HTTP status alone does not establish the
reason for a refusal. Execute a new dispatch only when explicitly directed; the
eight-send limit is per run, not a cross-run authorization ledger.
