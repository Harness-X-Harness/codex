# Facts command

This is the branch-local invocation mechanism for #338. It calls the existing
`grok/facts` probes; it does not add a new observation oracle or product policy.
It is independently buildable with the Go version in `grok/go.mod` and needs no
product package. From `grok/`:

```sh
go build -o grok-facts ./cmd/grok-facts
GROK_FACTS=1 ./grok-facts --scenario basic-primary --source-sha "$GITHUB_SHA"
```

The command consumes `GROK_API_KEY` from an already approved execution environment.
Do not put credentials in arguments, logs or source. Backend execution needs its
own authorization; building or testing this command does not authorize a call.
Ordinary native PR proof supplies no backend credentials and keeps opt-in disabled.

Select exactly one scenario: `basic-primary`, `basic-pinned`, `encrypted-replay`,
`shipped-routes`, `web`, `web-allowed`, `web-excluded`, `x`, `x-window`,
`web-replay`, or `x-replay`. Unknown or empty selections are errors. These names
map only to the already owned fixtures. Shipped routes consume the embedded
profile/catalog; other scenarios retain `internal/providerfixture` controls.
No invocation retries a failed observation to manufacture success.

The default endpoint comes from that selected fixture. `--endpoint` explicitly
selects another Responses endpoint; redirects remain prohibited. HTTPS is required
except for literal loopback IPs used in deterministic HTTP tests. Endpoint userinfo,
query and fragment are rejected. The report includes only its SHA-256 fingerprint,
never the URL. An explicit endpoint is not automatically an authorized destination
for the supplied credential or evidence of real-provider execution.

`--timeout` bounds the whole observation, including continuation/route requests;
the default is two minutes and the maximum is three minutes. The Probe's existing
per-request limit also applies. Interrupt/deadline failures retain safe partial
metadata, and do not become completed observations.

One JSON object is written to stdout. Exit 0 means the selected observation
completed according to its existing Probe; 1 means not observed or report-write
failure; 2 means invalid arguments/configuration, including missing credentials
after opt-in; 3 means not run because explicit opt-in was absent. `--help` exits 0
without an observation report or any network call. Missing/empty/skipped execution
therefore cannot supply a successful observation object.

The JSON retains safe completion/stage/status/count metadata, scenario identity,
UTC recording time, and the caller-declared source SHA. Source provenance is not
established by accepting a SHA argument: the Actions caller must independently bind
checkout, workflow/harness revision, command and selected endpoint. Request counts
are transport-attempt diagnostics, not proof of backend receipt or result correctness.
HTTP 401/403 are labeled authentication-or-access rejection without guessing a cause.
Changed or incomplete responses remain `not_observed`; #324 owns their classification.
Returned model aliases remain diagnostic as in the original probes.

The command's integration tests compile and execute the actual binary against
isolated loopback servers, with synthetic credentials and responses. They exercise
scenario/policy routing, complete and partial results, no-invocation preflight,
redaction, redirects and interruption. The existing shared `grok-proof.yml` runs
these tests alongside every existing Facts/Live test with backend opt-ins disabled.

## Actions invocation

Formal acceptance evidence comes only from GitHub Actions. Local command/test
results are development diagnostics. The separate `grok-facts.yml` workflow has
no native `Cargo` job and does not depend on product package builds.

Relevant version-line pushes select all 11 retained observations only when the
execution context exposes the existing `GROK_FACTS=1` repository variable. A
missing opt-in leaves the backend jobs skipped; that is not successful Facts
execution. No step creates a variable or configures credentials. The ordinary
PR/push native workflow still runs all deterministic Facts/CLI tests with no
backend opt-in or secret.

Manual invocation selects one named observation on the explicit version ref.
Once that ref contains this workflow, the intended operator can submit:

```sh
gh workflow run 359407907 --repo Harness-X-Harness/codex \
  --ref grok/rust-v0.158.0 -f scenario=basic-primary
```

The ID is the existing repository registration for `.github/workflows/grok-facts.yml`;
its current availability must be checked rather than inferred from a YAML trigger
or registry state. A historical dispatch failure remains failure evidence until an
actual new submission and matching run are verified. The workflow admits only this
repository's version-line refs. An explicit manual request supplies the original
`GROK_FACTS=1` execution context, and only the invocation step receives the already
configured backend secret. Checkout/workflow/source, run/attempt, scenario and safe
result JSON are retained for 30 days. Matrix fail-fast is disabled; observations
are not automatically retried.

A submitted run is not a completed observation. Missing credentials, skipped jobs,
empty/invalid selections, changed responses and failed observations cannot become
backend passes. #324 owns interpretation and freshness of actual Facts results.
No default-branch edit, registered-entrypoint success, product source promotion,
Live result or complete #338 acceptance follows from source publication alone.
