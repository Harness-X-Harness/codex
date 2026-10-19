# Grok contract- and test-driven development

This is the **current** Grok development policy. The filename `carry-forward.md`
is retained for existing links; it does not prescribe a separate carry branch,
migration pipeline, or promotion system. [#233](https://github.com/Harness-X-Harness/codex/issues/233)
owns the acceptance rule: **an actual branch HEAD is acceptable when all
applicable required CI for that exact SHA succeeds**.

C0–C8, Green Prefix, `RECONSTRUCTED`, canonical-squash certification, and
mandatory post-merge proof were useful historical development practices, **not
current phases, acceptance states, or test selectors**. Their historical
decisions/results remain in Git and Issues, not in a replacement roadmap.
There is no universal number or required order of implementation increments.

## Authorities and boundary

| Authority | What it owns |
| --- | --- |
| Trusted upstream stock tag/commit | Pinned starting source, stock architecture, existing behavior, native tests and generators. Trust in stock is a premise, not downstream proof after modification. |
| Current product Stories on the version branch | Consumer-visible positive/negative behavior and deliberate support or retirement decisions. |
| Rust/Go source and its adjacent tests | Current implementation and executable safety/behavior contracts; stock regression tests remain with their native owner. |
| `grok/internal/scenarios/catalog.go` on the version branch | Names, selection and invocation budgets of Go-owned Live/Facts scenarios; workflows do not maintain a second catalog. |
| `grok/TESTING.md` on the version branch | Actual local/test-tier commands and interpretation of evidence. |
| Git commit and GitHub Actions | Tested source identity, applicable checks, run/attempt outcomes and artifacts; never a source-controlled success ledger. |
| `grok/main` | Process documentation and useful active experiments, not a distinct accepted product or mandatory promotion branch. |

A new `grok/rust-vX.Y.Z` line can be created at the **exact pinned stock
commit** by an authorized operation; read back the ref and inspect protections
actually configured. The previously shipped Grok version is an input for
identifying retained user behavior, not a special promotion authority. Work
may proceed on ordinary feature branches and through PRs to the appropriate
product branch; no particular working-branch name is inherently authoritative.
Do not reset, force-push, bypass protections, or rewrite a protected line
without separate authorization.

## The ordinary development path

1. **Define a change in behavior, not a phase.** Use an existing Story or
   concrete bug/Issue. State the observable success and failure, safety
   boundaries, and non-goals. For a pure refactor, preserve the current
   contract rather than inventing a new one. A support-policy change needs an
   explicit reviewed product decision.
2. **Find its current owner and real dependencies.** Examine the relevant
   stock seam, callers, config, protocol/schema, history, tools, generated
   representations, and actual consumers. Trusting untouched stock does not
   excuse regressions caused by our delta. Read history only when a current
   invariant cannot otherwise be established.
3. **Make the contract executable at the lowest useful layer.** For new or
   broken behavior, add a focused failing regression/positive test before
   implementing when practical; for cleanup, verify existing positive and
   material negative oracles before removing anything. Keep Rust internal
   correctness in Rust, Go observation in Go, real-Codex composition in
   Integration, and real backend behavior in explicitly authorized Live/Facts.
   A mock, skip, or expected-unsupported diagnostic is never a positive
   product Story.
4. **Implement the smallest complete change.** Update the owning source,
   affected consumers, config, schemas/generators, build inputs, documentation
   and tests together. Delete replaced behavior and its dead paths rather than
   adding compatibility layers or stage scaffolding. Split work only at an
   independently useful, tested **technical** dependency; otherwise one
   coherent PR is preferable. No C0–C8 serial order.
5. **Verify locally, then on the actual HEAD.** Run the affected tests,
   validations and checks, broadening for shared or uncertain impact. Keep
   fail-closed tests for authorization, identity, external effects, history,
   resource bounds and confidentiality. Before accepting a code change,
   obtain settled, nonempty, applicable required CI on its actual source HEAD.
   A PR synthetic merge SHA is a different subject and must not be relabeled
   as feature-HEAD proof.
6. **Review and merge normally.** Inspect changes and evidence using
   [review.md](./review.md); follow the repository's real branch protection
   and ordinary Git operations. A squash/rebase changes the commit identity,
   so its resulting HEAD receives the ordinary applicable CI when triggered;
   this is **not** a new post-merge certification or pause before further
   feature development. Do not add a release-state machine.

A successful test proves only its own assertion, source, executable and
environment. Genuine scope gaps remain explicitly `NOT_RUN`/unproven until
the correct layer has actually observed them. Do not infer facts from
historical green runs, completed setup, or workflow YAML.

## Tests selected by changed behavior

| Boundary | Responsible checks | What a pass does not establish |
| --- | --- | --- |
| Go observer and deterministic contracts | `go -C grok test ./...`; affected `-race`, vet, formatting, fixture and result-checker controls | Rust composition or real Grok backend |
| Rust Provider, tool, history, App Server, stock seam | Affected native Rust tests, lint and owned protocol/schema/generator/stock regressions | Real Codex/Go consumer or paid provider outcome |
| Real Codex + controlled Grok HTTP | `go -C grok test -tags=integration ./tests/integration` with an explicit built executable | Actual xAI responses, unrelated product Stories or packaged target execution |
| Real Grok product | Authorized `-tags=live ./tests/live`, appropriate selected scenario and exact binary/artifact provenance | Unselected scenarios, other platforms, all stock contracts |
| Direct backend facts | Authorized `-tags=facts ./tests/facts` | Codex product behavior or acceptance |
| Packages and distribution | Native target builds, packaging tests and appropriate actual package consumers/smoke | Other platform execution, unrun Live or a separate source promotion |

These paths describe ownership, not an obligation to run every tier on every
change. Use the target branch's `TESTING.md` and workflows for supported
flags, authentication, subject verification and exact commands. The CI
applicability mechanism is orchestration only: it must not mask a required
native test, silently choose an empty suite, or grant credentials to
untrusted PRs. If a broad shared seam changes, broader Rust/Go/Integration
and affected stock proof is appropriate. Remove redundant tests only when
the preserved invariant has equal or stronger coverage.

## Updating to a newer stock tag

Pin and verify the requested upstream `rust-vX.Y.Z` commit; start the target
version line from that exact stock when a new line is actually needed. Use
current Stories and the existing regression tests to discover the smallest
required downstream adaptations. Do not begin by reconstructing C0–C8,
transplanting an old downstream tree, replaying historical patch counts, or
building a second test harness.

Fix incompatibilities with scoped tests and coherent PRs. Actual dependencies
may impose ordering (for example, protocol compatibility before a consumer
can compile), but incidental historical landing order does not. Where stock
now owns behavior, prefer stock and retire redundant Grok code. Update a
Story only when consumer behavior or its supported scope intentionally
changes; changing a fixture or adapting to stock is not automatically a new
product requirement.

## Evidence, delivery and maintenance

Maintain the distinction between source SHA, binary SHA-256, PR merge
subject, run/attempt, package target and backend environment where they
affect reproducibility. Required checks must be actually selected and settled
successfully; pending, failed, cancelled, skipped, stale, neutral or empty
required checks are not green. Source/branch names, old proof records and
metadata-only claims are not tests.

Actual external Grok API calls need explicit opt-in and trusted credentials.
Facts, real-provider Live, and package smoke establish different things;
their availability is not evidence they ran. Distribution is optional and
belongs to [distribution.md](./distribution.md), not to an extra source
acceptance state. Changes with external effects need correct target binding,
truthful residual-effect handling and authoritative readback; do not blindly
retry non-idempotent actions.

GitHub Issues coordinate unresolved outcomes and genuine dependencies,
not a numbered release stage plan. Keep the existing Story index and test
owners as the only functional acceptance map; do not create another
acceptance ledger, stage manifest, proof runner, prefix label, or recurring
document of the latest green SHA. Preserve historical failures as historical
evidence, while fixing any **current** behavior or policy they still affect.
