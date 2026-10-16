# Grok test tiers

Run commands from the repository root with the Go version in `grok/go.mod`.
[Story ownership](docs/stories/README.md) and the
[test-layer inventory](docs/test-layer-inventory.md) distinguish the evidence
produced by each tier. None of the commands below promotes a local build to a
verified release or refreshes historical Live results.

## Default: offline Go contracts

```sh
go -C grok test -race -count=1 -timeout=15m ./...
go -C grok vet ./...
go -C grok mod tidy -diff
test -z "$(gofmt -l grok)"
```

No Grok credentials, compiled Codex, external backend or GitHub context is
needed. Tests use fake App Server subprocesses and local HTTP fixtures. The
shared `TestMain` helpers still own child launch and cleanup. Package staging,
launcher and artifact tests remain Python contracts:

```sh
python3 grok/package/test_stage.py -v
python3 grok/package/test_diagnose_linux_binary.py -v
python3 grok/package/test_select_artifact.py -v
python3 grok/package/test_extract_artifact.py -v
python3 grok/package/test_fetch_artifact.py -v
```

These five Python files currently contain 57 tests; they use synthetic inputs
and do not replace native package smoke.

## Integration: actual Codex and controlled Grok HTTP

Select an existing absolute executable. There is no implicit build, download,
PATH search or backend call:

```sh
GROK_CODEX_BIN=/absolute/path/to/codex \
  go -C grok test -race -tags=integration -count=1 -timeout=20m \
  -run '^TestIntegration' ./live
```

The wrappers live beside their existing private test helpers to avoid copying
business assertions into another test package. `TestIntegration` selects Basic,
structured edit, reasoning history, hosted search, image-pinned and shipped
catalog. `TestIntegrationInternal` selects shipped source, child and first-request
shape. A single already-built binary serves all nine owners. To select one:

```sh
GROK_CODEX_BIN=/absolute/path/to/codex \
  go -C grok test -race -tags=integration -count=1 \
  -run '^TestIntegration$/BasicFixture$' ./live
```

`GROK_LIVE_NATIVE_BIN` remains a compatibility alias; conflicting paths fail.
Missing, relative, non-executable, malformed or wrong-platform binaries fail the
required run rather than skipping it. The runner hashes the chosen executable,
checks the consumed bytes, and uses isolated temporary home/workspace/credentials
and owned process cleanup. Without caller-established source/harness provenance,
the subject is explicitly local/unverified; dirty development work is not
represented by invented SHA values. CI continues to bind native runs to the
exact `GITHUB_SHA`; package tests retain their exact source, target and digest.

The unmodified public X notification stream is required. The pinned SDK's
unsupported `xSearch` variant is a compatibility gap tracked by
[#408](https://github.com/Harness-X-Harness/codex/issues/408). It may make
`HostedSearchFixture` fail. Do not filter stdout, skip the X cases, loosen SDK
validation or report an offline fake-process result as a passing original-wire
Integration result.

## Live: actual Codex and actual Grok

Only execute these commands when real-backend use is authorized. Credentials
come from the environment; never place a key in arguments or saved evidence.
The selected binary is required even for catalog-only observation.

```sh
# The shell already has GROK_API_KEY; this is one explicit diagnostic case.
GROK_LIVE=1 GROK_CODEX_BIN=/absolute/path/to/codex \
  go -C grok test -tags=live -count=1 -timeout=10m -json ./tests/live \
  -args -grok-live-suite diagnostic -grok-live-scenario basic-primary

# Explicit economical selection, still real backend execution.
GROK_LIVE=1 GROK_CODEX_BIN=/absolute/path/to/codex \
  go -C grok test -tags=live -count=1 -timeout=25m -json ./tests/live \
  -args -grok-live-suite smoke

# Every formal catalog scenario; sequential, no semantic resubmission.
GROK_LIVE=1 GROK_CODEX_BIN=/absolute/path/to/codex \
  go -C grok test -tags=live -count=1 -timeout=100m -json ./tests/live \
  -args -grok-live-suite full
```

If `-grok-live-suite` is omitted, the Go entry point selects smoke. The catalog
discovery command separately defaults to full. An explicit `-count=1` is required
to prevent a cached backend success from standing in for a new invocation.

Stable selectors are `TestLive/<scenario>`. `-run '^TestLive$/web$'` is supported;
unknown/empty selections, missing opt-in/key/binary, skips and incomplete selected
cases must fail. Do not use repeated `-count` or multi-value `-cpu` to resubmit
paid work. Each scenario has a five-minute limit and invokes its existing
business owner once. Local subjects remain local/unverified.

[`internal/scenarios/catalog.go`](internal/scenarios/catalog.go) is the only
invocation catalog, shared by Go tests, the compatibility CLIs and CI matrices:

```sh
go -C grok run ./cmd/grok-scenarios --family live --suite smoke
go -C grok run ./cmd/grok-scenarios --family live --suite full
go -C grok run ./cmd/grok-scenarios --family live --suite diagnostic
go -C grok run ./cmd/grok-scenarios --family facts --suite full
```

The discovery command is credential-free and prints a JSON name array. Live
smoke contains primary Basic, primary reasoning, Web and X: four cases, at most
seven submitted semantic turns. Full retains 19 cases and at most 30 submitted
semantic turns. Catalog-only submits zero inference turns. Diagnostic discovery
also lists `reasoning-no-effort-pinned`; its execution requires an explicit name.
That exploratory control is excluded from smoke/full and formal release proof.
These are invocation bounds, not observed API billing or limits on legitimate
product-internal transport/tool activity. No measured paid-cost improvement is
claimed.

Trusted workflows first use `package/fetch_artifact.py` to verify the archive,
provenance, source/harness revisions, target and executable digest. They pass
`-grok-live-verified-subject`, `-grok-live-binary-sha256`,
`-grok-live-source-sha`, `-grok-live-harness-sha`, `-grok-live-target` and
`-grok-live-environment` to the same Go entry point. Merely setting those flags
locally is not independent proof of package origin.

## Facts remain independent

Direct backend observation does not launch Codex or prove a product Story.
The existing `grok-facts` CLI and workflow keep their own opt-in, budget,
request/replay assertions and JSON shape. Explicit backend Go tests use the
`facts` build tag:

```sh
GROK_FACTS=1 go -C grok test -tags=facts -count=1 ./facts
```

This is real backend use and needs an already-authorized `GROK_API_KEY`.
The default build excludes these entry points. A selected Facts run with missing
opt-in or credentials fails. See [Facts](facts/README.md) for policy-admission
limits; a successful response does not establish relevance or policy enforcement.

## JSON evidence and CI routing

Capture Go events, retain the test exit status, and check completion even when
the test command fails:

```sh
results="$(mktemp)"
status=0
GROK_CODEX_BIN=/absolute/path/to/codex \
  go -C grok test -race -tags=integration -count=1 -timeout=20m -json \
  -run '^TestIntegration' ./live > "$results" || status=$?
go -C grok run ./cmd/grok-test-results --input "$results" --integration || status=1
exit "$status"
```

`grok-test-results` checks JSON lifecycle events, not human output text. It
rejects failed, absent, skipped or incomplete required tests and an empty suite;
Go package-only `[no test files]` events are not test skips. `--integration`
requires all nine named owners. Repeated `--require package:Test/subtest` supports
an exact partial package or Live selection. Never infer coverage from a green
package containing no selected tests. A partial selection does not prove omitted
cases passed. Per-case Live evidence retains safe counts, subject/digest,
provenance and failure classifications, without prompts, model text, ciphertext,
keys, private paths, raw traffic or opaque IDs.

- `grok-ci.yml` runs credential-free checks on Grok-related development pushes
  and PRs outside the existing formal bases. `grok-go.yml` owns format, tidy,
  race tests, JSON checks, vet, Python contracts and clean-worktree checks.
- Formal PR bases and canonical version-line pushes call the same Go workflow
  through `grok-proof.yml`; their merge subject is not checked twice by
  `grok-ci.yml`. The runtime job preserves every Rust owner and runs only tagged
  Go Integration after its existing CLI build.
- `build-grok` keeps its five existing package consumer owners, selected as
  Integration subtests; it does not repeat the offline Go suite.
- Formal Live derives the full matrix from Go, keeps at most three jobs active,
  and preserves trusted repository/ref/opt-in and same-run artifact checks.
- Manual `grok-live.yml` defaults to one `basic-primary` diagnostic. Smoke/full
  are explicit complete selections; their scenario text field is ignored.
  Diagnostic runs do not become release certification.
- `grok-facts.yml` derives its separate matrix from the Facts catalog and remains
  independently optional. Neither Facts nor Live replaces the protected `Cargo`
  aggregate. Failure JSON and summaries are retained even when tests fail.

Workflow source and syntax checks are not executed Actions evidence. Current
measurements and remaining unrun boundaries belong in the review/run record.
