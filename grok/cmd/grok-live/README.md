# Explicit-binary Live command

`grok-live` makes the existing `grok/live` scenario functions independently
invocable. Build from `grok/` with `go build -o grok-live ./cmd/grok-live`.
`--help` lists the 19 existing primary/pinned, reasoning, edit, image, shipped
profile/child and hosted-search selections. The command adds no product oracle.

One invocation selects one scenario. Provide an absolute immutable binary,
its expected SHA-256, source SHA, harness SHA, supported package target and
environment label. The caller establishes source/target provenance, native
eligibility and artifact identity. The underlying runner verifies the binary
digest and owns isolated Home/workspace creation, process cleanup and observation.
Package callers first verify the selected archive and complete package with
`grok/package/fetch_artifact.py`, which composes the exact selection and
archive-extraction validators.

Real execution uses the existing `GROK_LIVE=1` opt-in and an already approved
`GROK_API_KEY` environment. Neither building the command nor a successful
artifact verification authorizes a backend invocation. Do not place secrets in
arguments, environment labels, logs or source. No new authorization switch or
credential configuration is introduced here.

The fixture endpoint defaults to the existing Provider fixture. HTTPS is
required except for literal loopback addresses used by deterministic tests.
Shipped scenarios use the exact profile-owned endpoint and reject an override
instead of silently ignoring it. The whole scenario deadline defaults to five
minutes and cannot exceed the underlying ten-minute bound. The command never
restarts a scenario or resubmits a failed semantic turn.

One JSON result contains the selected scenario, UTC time, endpoint fingerprint
and unchanged safe evidence returned by its owner. Exit 0 is a scoped observed
result, 1 is not observed or output failure, 2 is invalid configuration/subject,
and 3 is not run without opt-in. `shipped-catalog` records catalog observation
without inference completion. Partial facts remain separate from overall
completion; failures contain static reasons and no raw child/backend traffic.

The command tests compile the real CLI and the existing Live package's fake
App Server executable. They run every selection through actual child processes,
check subject/model/turn binding, and exercise preflight, failed-scenario,
whole-deadline, actual interrupt, help and output-write behavior. These are
deterministic mechanism tests, not actual Grok package or backend results. Formal evidence remains the identified GitHub Actions run.
Independent invocation does not close #339's build-to-Live or actual manual
registration requirements, nor #331's retained product Stories.
