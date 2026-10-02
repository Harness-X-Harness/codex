# Explicit-binary Live scenarios

`Basic(ctx, Options)` consumes the selected binary's App Server v2 stdio API:
initialize, one fixture-bound thread, one text turn, matching completion and a
nonempty final reply. Primary/pinned fixtures use `internal/providerfixture`
controls and an isolated text-only profile/catalog, independent of `grok/dist`.

Supply an absolute binary path and expected SHA-256, caller-established source,
harness/target/environment metadata, model, Provider base URL and key. The binary
must remain immutable during the invocation. A deadline of at most ten minutes
is required. The runner never restarts/resubmits; product retries retain their
policy. Frames cap at 16 MiB, early traffic at 128 frames/8 MiB. Cleanup closes
stdin, allows bounded shutdown and stops/reaps the child when needed.

Returned evidence contains safe declared metadata, time, counts, booleans,
reply byte count and last stage. Errors contain no private traffic; failure
attribution remains inconclusive without independent evidence. Success observes
only this path; source/target provenance and native eligibility are prerequisites
for a Story claim, supplied by callers and #339's process integration.

CI tests the public stdio boundary deterministically and activates
`TestNativeBasicFixture` with its freshly built CLI. Controlled HTTP proves
config/request/stream composition, not real backend Live. Actual execution and
invocation integration remain #331/#339.

`ReasoningHistory(ctx, Options)` shares Basic's artifact/process/fixture binding
and adds the [encrypted-history Story](../docs/stories/grok-encrypted-reasoning-history-continuation.md).
It opts into raw events and one named dynamic tool, with explicit medium effort.
Only a valid first-turn tool request creates the in-memory 32-byte random token;
repeated calls return the same token. Completed reasoning, opaque encrypted
output, a successful completed named tool and final token recall must all match
turn N before a single fixed, token-free continuation is submitted. N+1 must
complete and recall that token; tool requests receive a token-free denial while
observation continues. No private token, ciphertext, text or identity is returned.

Per turn, reply evidence retains at most 4096 assistant-ID SHA-256 digests and
final-eligibility/recall bits. Consistent duplicates cannot revive a superseded
reply; conflicting duplicates prevent proof until an authoritative terminal
assistant summary reconciles them. A previously unseen delayed final remains
eligible when that summary is absent. Missing assistant IDs and identity-budget
overflow fail with static errors. This is a harness memory bound, not a limit on
product tool calls or retries. Authoritative summaries bypass and release the
digest map; provisional evidence starts fresh for the continuation.

Deterministic public-stdio tests cover ordering, correlation, terminal evidence,
secret-safe failures and invocation budgets for both model controls. Actual
native tool/HTTP composition and real-provider execution require #322; the
current product rejects nonempty Grok tools. This harness increment does not
establish backend success, unchanged native encrypted replay, or Story PROVEN.
