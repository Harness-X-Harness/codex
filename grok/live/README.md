# Explicit-binary Live scenarios

`Basic(ctx, Options)` consumes the selected binary's App Server v2 stdio API:
initialize, one fixture-bound thread, one text turn, matching completion and a
nonempty final reply. Primary/pinned fixtures use `internal/providerfixture`
controls and an isolated text-only profile/catalog, independent of `grok/dist`.
The Basic thread selects no execution environments or their contributed tools.

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
invocation integration remain #331/#339; reasoning continuation is C1c2b of #320.
