# Grok test tiers

Run commands from the repository root with the Go version in `grok/go.mod`.
[Story ownership](docs/stories/README.md) distinguishes consumer guarantees and
their code/test owners. Each tier below produces different evidence. None of
the commands below promotes a local build to a verified release or refreshes historical Live results.

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
  ./tests/integration
```

The actual scenario implementations live in `tests/integration`, with nine
independently selectable owners: `TestBasicFixture`, `TestStructuredEditFixture`,
`TestReasoningHistoryFixture`, `TestHostedSearchFixture`, `TestImagePinnedFixture`,
`TestShippedCatalog`, `TestShippedThreadSource`, `TestShippedChildFixture` and
`TestFirstRequestShape`. No wrapper forwards into `live/*_test.go`. Shared
fixture mechanics live under `internal/integrationfixture`; production observers
and credential-free white-box tests remain in `live`. One built binary serves
all owners. Select one with `-run '^TestBasicFixture$' ./tests/integration`.

`GROK_LIVE_NATIVE_BIN` remains a compatibility alias; conflicting paths fail.
Missing, relative, non-executable, malformed or wrong-platform binaries fail the
required run rather than skipping it. The Integration and Live/CLI entry points
verify the selected executable's copied bytes and launch an owned snapshot, with
isolated temporary home/workspace/credentials and owned process cleanup.
Without caller-established source/harness provenance,
the subject is explicitly local/unverified; dirty development work is not
represented by invented SHA values. CI continues to bind native runs to the
exact `GITHUB_SHA`; package tests retain their exact source, target and digest.

The private snapshot protects execution from replacement of the original input
path, including replacement followed by restoration between digest checks. Its
files are non-writable. Successful joins remove the copy; unproved startup or
shutdown cleanup retains both snapshot and isolated fixture, reports
`CleanupPending` and `execution_snapshot_retained`, and requires confirming
termination before manual cleanup. Each retained copy remains subject to the
limits below; repeated failed invocations can accumulate copies. Integration
tests conservatively retain their snapshot on failure; expected-negative tests
also require a proven join before passing. Canonical Grok
packages retain their manifest-listed launcher, runtime, helpers and resources
at the same relative paths. Raw development executables retain their basename
and an optional adjacent `codex-code-mode-host`; arbitrary build directories are
not copied. Admission rejects invalid manifests, path traversal, source-root
symlinks and special files, with limits of 128 files, 1 GiB per file, 2 GiB total
and 1 MiB metadata. Subject digests and caller-established provenance do not
change when the launch location changes.

The owned directory must remain trusted. This is protection from ordinary
source-path replacement, not from a malicious process running as the same user.
A raw helper copy does not establish an atomic multi-file build or release
origin. Direct calls to the low-level `live` functions still require the caller
to keep their supplied executable stable; those functions additionally verify
its digest before launch and after cleanup. Synthetic snapshot and caller tests
do not replace actual-binary Integration acceptance for
[#404](https://github.com/Harness-X-Harness/codex/issues/404).

The original App Server stdout reaches the pinned SDK unchanged. Supported Web
requires a canonical hosted call, completed answer, persisted typed history and
a completed same-thread follow-up. See the
[Hosted X retirement boundary](docs/stories/README.md#hosted-webx-search)
for the product exclusion, old-history disposition and the separate
Rust emitted-stream/schema and offline SDK diagnostic owners.

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

The discovery command is credential-free and prints a JSON name array. The
catalog owns selected cases and per-case semantic-turn limits; derive those
limits from its current entries instead of copying counts into documentation.
Catalog-only submits zero inference turns. Diagnostic execution requires an
explicit name. See the [Hosted X retirement boundary](docs/stories/README.md#hosted-webx-search)
for unavailable product cases and separate direct-backend Facts. Invocation
bounds are not measured API billing or limits on legitimate product-internal
transport/tool activity. No paid-cost improvement is claimed.

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
GROK_FACTS=1 go -C grok test -tags=facts -count=1 ./tests/facts
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
  ./tests/integration > "$results" || status=$?
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
keys, private paths, raw traffic or opaque IDs. The tagged aggregate reports
`observed` only when every selected supported case passes its positive oracle.
Its `completed` count counts completed cases; `shipped-catalog` remains an
explicit zero-inference catalog case. SDK rejection diagnostics never enter the
formal aggregate or earn completed-case credit.

### Live failure evidence

[`internal/livecommand`](internal/livecommand/run.go) owns the bounded report
shared by [`TestLive`](tests/live/live_test.go) and the compatibility CLI.
[`live.FailureInfo`](live/failure.go) projects only allowlisted failure code,
terminal status, error kind and a valid HTTP status when the supported typed
terminal actually supplies it. Scenario evidence supplies stage, safe counts,
booleans, hashes and source/harness/target provenance. Unknown or unavailable
facts stay unknown or absent; neither an HTTP status nor a static failure code
proves the rejection's root cause. Request bodies, rejected-request key-path
shapes, backend error bodies and session JSONL are not collected by this path.
A report is capped at 1 MiB including its newline. In-memory identity and content
correlation does not authorize storing those values in the report.

The per-case outcomes remain `observed`, `not_observed`, `not_run` or
`configuration_error`; the tagged aggregate is `observed` only for a complete
positive selection and otherwise `not_proven`. Failure, cancellation and missing
configuration never become successful backend proof. An actual diagnostic
observation remains scoped to its selected subject and is not release certification.
No missing evidence authorizes semantic resubmission or paid inference.

The existing [formal workflow](../.github/workflows/grok.yml) and
[diagnostic workflow](../.github/workflows/grok-live.yml) retain Go test events,
the checked summary and verified package selection/provenance in the existing
Actions artifact store, including on failure when files exist. Their configured
retention is 30 days, under the repository's existing Actions artifact access
controls; this change grants no new access. Artifact names bind SHA, run and
attempt (plus scenario for formal runs). Formal package selection is same-run;
diagnostics retain the explicitly selected producer identity separately from
the harness run. Trusted repository/ref checks and secret isolation from
untrusted PRs remain in those workflows. Inspect the actual run/attempt and
artifact metadata and read back the selected files before claiming delivery;
workflow configuration alone proves neither upload, current availability nor
expiry. A local output file has no automatic store, access or expiry guarantee.

### CI routing

- `grok-ci.yml` runs credential-free offline checks on development branch HEADs.
  Relevant native/schema changes additionally reuse `grok-proof.yml` on that
  exact push subject. Applicability and aggregate success fail closed. Push
  applicability uses the branch diff from the default-branch merge base, not
  just the last commit: a docs follow-up cannot hide unvalidated native changes.
  This conservative base can still require native checks for a Grok fork
  docs-only follow-up; no current-branch cost reduction is claimed. `grok-go.yml` owns format, tidy, race, JSON, vet
  and Python checks, once per selected workflow subject.
- Manual `grok-ci.yml` dispatch selects a branch HEAD without arbitrary SHA
  inputs and requests native checks. GitHub requires that dispatch workflow to
  be registered on the default branch first; ordinary feature-push execution
  supplies the pre-merge route.
- Version-line/PR native checks retain their existing `grok.yml` entry point.
  A PR synthetic merge SHA is labeled separately from its feature HEAD. Native
  jobs preserve scoped Rust owners and invoke only `tests/integration` after
  the existing CLI build. GNU native proof does not certify Linux-musl/macOS
  release packages; their existing applicable `grok.yml` jobs remain separate.
  No branch rules or post-merge certification changes.
- `build-grok` keeps its five actual package consumer owners in
  `tests/integration`; it does not repeat the offline suite.
- Formal Live runs automatically on every trusted version-line push after the
  successful Linux build and scenario selection, making real API calls and
  incurring their costs without a repository-variable opt-in. It derives the
  supported full matrix from Go, keeps at most three jobs active, and preserves
  trusted repository/ref and same-run artifact checks.
- Manual `grok-live.yml` defaults to one `basic-primary` diagnostic. Smoke/full
  are explicit complete selections; their scenario text field is ignored.
  Diagnostic runs do not become release certification.
- `grok-facts.yml` derives its separate matrix from the Facts catalog and remains
  independently optional. Neither Facts nor Live replaces the protected `Cargo`
  aggregate. Failure JSON and summaries are retained even when tests fail.

Workflow source and syntax checks are not executed Actions evidence. Current
measurements and remaining unrun boundaries belong in the review/run record.
