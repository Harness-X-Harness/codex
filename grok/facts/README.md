# Basic and reasoning backend observations

This #320 foundation observes the configured Grok HTTP Responses endpoint.
It does not exercise a product binary or complete a Live Story. The primary
`grok-4.7` and pinned `grok-4.6` controls come from the previous accepted source;
they are explicit harness fixtures, independent of future `grok/dist` assets.

Retain Basic/pinned request routing and encrypted-reasoning typed-history replay.
Search/tool policy probes enter with #322, images with #328 and shipped profile
checks with #329. Do not carry their unused helpers here. Historical accepted
observations do not demonstrate present backend behavior.

## Proposed assertion and evidence matrix

| Consumer | Minimum operations | Required observation | Safe evidence |
| --- | --- | --- | --- |
| Basic | one POST | Successful complete text response | status, completion, text byte count |
| Pinned model | one POST | Request uses its explicit fixture; complete text response | fixture label, status; returned-model match is diagnostic |
| Encrypted reasoning replay | one initial POST, one replay POST | Opaque reasoning observed, copied unchanged with typed content into replay; complete replay response | encrypted-item count, replay/completion booleans |

Never resubmit to manufacture a successful observation. Redirects cannot move
the probe to another endpoint. A failed first reasoning operation prevents the
replay. Bound response reads; malformed, oversized, partial or unreadable input
does not become successful observation. Parse supported response representations
by their semantic text/reasoning facts; unrelated fields are not outcome gates.

Backend tests require explicit `GROK_FACTS=1` and `GROK_API_KEY`. Missing opt-in
means not run; missing credentials after opt-in is failure. Required CI disables
backend opt-ins and executes meaningful local-server deterministic tests.

The HTTP observation requires nonempty assistant `output_text` and completion:
either a completed response, or (when response status is omitted) completed
assistant messages. Explicit partial/error evidence rejects either path; a bare
2xx, ciphertext or text without completion is insufficient. Returned-model
equality is diagnostic, so backend aliases do not reject a valid observation.
The replay probe combines one unchanged opaque reasoning value with synthetic
typed `reasoning_text`; it observes this backend representation, not product
plaintext-replay policy or semantic recall.

Publish only fixture/scenario labels, observation time, safe counts/booleans and
HTTP status. Never publish endpoint secrets, response bodies, ciphertext, model
output or opaque IDs. HTTP/backend observations do not establish product policy,
artifact provenance, native proof, or a PROVEN Live outcome. Missing completion
is not observed; root-cause attribution requires separate supporting evidence.

The tested public seam is the Facts probe against an HTTP server: request
routing, complete text observation, unchanged opaque replay, failure isolation,
bounded reads and secret-safe errors. The invocation workflow belongs to #338.
Live runner source is the next C1c increment, governed by the
[Story contracts](../docs/stories/README.md).

`Probe.ShippedRoutes` consumes request slugs from the embedded shipped catalog,
submits one Text observation per route and stops on failure. The existing backend
opt-in test obtains its endpoint from the shipped profile for this route group.
Returned backend model aliases remain diagnostics; they never change request slugs
or the runtime catalog. This adds consumed scenario source, not the independent
Facts invocation mechanism owned by C8/#338.
