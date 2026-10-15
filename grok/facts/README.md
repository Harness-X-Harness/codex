# Basic and reasoning backend observations

This #320 foundation observes the configured Grok HTTP Responses endpoint.
It does not exercise a product binary or complete a Live Story. The primary
`grok-4.7` and pinned `grok-4.6` controls come from the previous accepted source;
they are explicit harness fixtures, independent of future `grok/dist` assets.

Retain Basic/pinned request routing and encrypted-reasoning typed-history replay.
C7 retains the bounded hosted-search fixtures below. Historical accepted
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

## Retained hosted Web/X source (C7)

`SearchPolicy` submits one fixed fixture: `web`, `web_allowed`, `web_excluded`,
`x`, or `x_window`. The domain fixtures advertise only `reuters.com` allowed or
`example.com` excluded. The X window advertises `2026-08-01` / `2026-08-15`.
These are independent controls, never a combined allow/exclude policy. A
completed text response establishes admission of the exact request, not search
result relevance, domain enforcement, or date-bound inclusivity.

`SearchReplay` submits one search and then at most one continuation. A completed
assistant response and a supported completed Web call or one of the four named
X calls must precede replay. It preserves admitted call identity, status and
action/input fields; it adds no local output and rejects observed local execution.
Unknown names, missing required identities/action tags/X input, incomplete calls,
output-paired calls and missing final text cannot authorize continuation. Web
action query/queries/url/pattern details are optional: absent, null and empty
values remain valid when their present types match the supported action variant. The existing redirect ban,
60-second transport timeout, 8 MiB response cap and static errors apply. At most
64 calls of at most 64 KiB each are retained in memory; no raw traffic is returned.

`TestBackendSearchFacts` is separately selectable, requires `GROK_FACTS=1`, and
contains five one-POST policy controls plus two at-most-two-POST replay controls
(maximum nine POSTs, no semantic resubmission). Credentials after opt-in are
mandatory. This source change does not execute that test against a backend or
inherit any prior recorded classification. Required CI sets `GROK_FACTS=0` and
runs local HTTP tests for exact payloads, all four X names, replay fields, missing
and malformed evidence, failures, invocation counts and secret-safe metadata.
Native Rust tests own provider/default precedence, strict dates, the five-domain
bound and fail-closed product restrictions. These Facts do not substitute for
those product tests or prove a Live Story.
