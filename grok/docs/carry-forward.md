# Version-line-first development

> A new Grok version should be born the same way good software is built: one
> complete, tested, understandable improvement at a time, directly on the line
> that will live with it.

This is the canonical current Grok development policy. The filename remains for
existing entrypoints. Carry-forward is the initial series of ordinary development
PRs on trusted stock, not a separate promotion or stock-certification system.
These are requirements, not claims that a particular line has implemented them.

## Authorities and trust boundary

| Term | Meaning |
| --- | --- |
| Exact stock `S` | Pinned upstream `rust-vX.Y.Z` commit, accepted as the stock input. Upstream/stock authority owns its baseline CI health and certification. It supplies the target architecture, behavior, generators, and native test entrypoints. |
| `grok/main` | Current process documentation plus optional active experiments on practical upstream main; not the accepted product baseline. |
| Version line `grok/rust-vX.Y.Z` | Durable development branch created at `S`. It owns canonical development history from bootstrap. |
| Accepted product source | Exact version-line revision whose planned product and development-process closure have been reviewed and accepted. |
| Working branch | Disposable branch for one PR, including test-first experiments and fixups. |

Trusting `S` is an explicit engineering premise, not a claim of exhaustive
correctness or an assertion that downstream reran upstream CI. Verify stock
identity and provenance; do not confuse that with behavioral re-certification.
The downstream responsibility is `DeltaProof + CompositionProof`, conditional on
the trusted stock input. It includes preservation of stock contracts affected by
our changes. Trust in `S` does not transfer automatically to a modified system.

During initialization the new line is development authority, not yet the next
accepted product baseline. The previous explicitly accepted line remains that
baseline. Neither branch naming, branch creation, C0 acceptance, CI success
alone, nor an artifact grants product acceptance.

There is no normal `carry/` branch, long-lived RC source branch, final bulk
promotion, or special fast-forward admission service. Old carry/RC branches may
remain frozen as historical evidence. A candidate is identified by a PR or SHA.

## Proof obligations, not an append-only test list

An increment is a change together with its justification: implementation, owned
derived outputs, tests, and proof activation land together. For a canonical
prefix `P`, the downstream obligation set comprises:

- the development/admission mechanisms introduced or changed on the line;
- the behavior and representations introduced by its deltas;
- existing stock contracts those deltas can affect, including indirect effects;
- interactions among accumulated deltas and the stock seams they compose with.

A stock regression belongs here because a downstream change can affect its
contract, not because an earlier run happened to execute that test. Unrelated
upstream certification does not become a permanent downstream obligation.
Conversely, unchanged source files are not proof of non-impact: inspect callers,
shared dependencies, data flow, configuration, and runtime boundaries.

Proof obligations accumulate while applicable; individual commands need not.
Refactor or replace tests with equivalent or stronger obligation coverage.
Retire an obligation only through an explicit reviewed semantic/scope decision,
not because its test fails, is expensive, or was inconvenient to run. Record the
reason in the owning PR and update the small version plan when its boundary
changes. No executable obligation manifest, source-marker detector, or proof
ledger is required.

Choose native tests, lint, consistency checks, environments, and composition
coverage from that impact analysis. If impact is uncertain, investigate or
broaden proof conservatively; a narrow diff is not sufficient justification for
narrow tests. A whole-crate or whole-workspace run can be justified by an actual
shared seam, but is not automatically owed by every prefix. Follow the pinned
repository's native entrypoints and applicable development instructions.

Here, "proof" means reviewed, scoped engineering evidence. Passing tests are not
formal verification or evidence for obligations that were never identified.

## Before the first increment

Pin the previous accepted product reference and exact target stock. Read only
the behavior, owner seams, native proof, and development capabilities needed for
continuation. Prior implementations may be read and locally reused, but must be
adapted and proved at current stock seams; do not transplant a final tree and
call its old CI proof current.

For each relevant behavior or capability choose `KEEP`, `UPDATE`, or `DROP`, with
rationale and an owner. Use stock when it now owns the behavior. Separate product
changes from representation cleanup. A verified baseline rewrite may aid reading,
but does not replace exact stock as the new branch's root.

Plan independently meaningful increments by real dependencies. An owner is not
necessarily one commit: split or combine only when each resulting increment is
complete and independently testable. Tests, CI, Facts/Live harnesses, review, and
optional delivery capability are development assets, not terminal cleanup.

## C0: establish the contract and proof path

> C0 establishes the version-specific development contract and the native
> admission/proof path. It does not re-certify exact stock.

C0 is `version contract + admission/proof plumbing + proof architecture
bootstrap`. Record a small contract under the version line's `grok/docs/`:
pinned inputs and trust boundary, decisions, real dependencies, intended
increments and their minimum obligations, and Facts/Live/package roles. Keep
run IDs and current scheduling in GitHub, not in an executable stage manifest
or a source-controlled CI-state mirror.

The reusable workflow may start with a thin, meaningful bootstrap job that
checks its execution subject and validates the C0-owned orchestration. No full
workspace Cargo tests or Clippy are required merely to make C0 substantial.
It must not modify stock product code, fixtures, workspace versions, or lockfiles
to obtain a green stock baseline. Do not introduce unused future-owner harnesses.

A YAML skeleton or a permanently successful no-op does not complete C0. Accept
C0 only with both structural review and actual platform evidence:

- a real PR calls the local reusable workflow and obtains the intended required
  `Cargo` from the expected source; active protection actually constrains admission;
- subject identity, event coverage, least privilege, dependency aggregation, and
  concurrency are reviewed, with non-success behavior verified;
- at least one applicable negative-path execution demonstrates that non-success
  in required inner proof cannot produce successful admission; cover remaining
  failure/missing/skipped/cancelled cases by explicit topology review and focused
  mechanism checks where feasible, not by inventing product tests;
- the reviewed final candidate has fresh applicable successful PR proof, is
  squash-merged normally, and its actual canonical SHA passes the same-definition
  push proof. Temporary negative-test changes must not remain as artificial gates.

Earlier mechanism evidence may support unchanged invariants only when its
applicability is explained. A changed candidate still requires its own PR proof;
changed mechanisms require corresponding revalidation. C0 acceptance establishes
a development mechanism, not stock correctness or Grok product acceptance.

### Stock failures and adaptations

Retain observed failures and classify their relevance. An unrelated pre-existing
stock CI failure is not automatically a C0 or downstream blocker. Runner inputs
needed by a later owner's legitimate proof belong to the first increment that
needs them, or to an independently complete proof-infrastructure increment.

If a concrete stock source, fixture, or lock problem blocks planned downstream
work, create an explicitly scoped stock-adaptation increment with a named owner,
reason, minimal diff, and affected regression/consistency proof. It uses the same
PR/squash/push loop and is not smuggled into C0. A stock adaptation is a downstream
delta even when mechanically small. Native generators close the representations
it actually changes; unrelated generated-file sweeps remain out of scope.

Trust is not a license to ignore evidence: a demonstrated defect that invalidates
a premise we rely on must be triaged. Nor does seeing an unrelated upstream
failure transfer ownership of all stock certification to the downstream line.

## Start the line safely

Create the version line at exactly `S`, not at an unreviewed Grok snapshot.
Read back the ref. Configure native protections before product work: PR-required
updates, strict required `Cargo`, squash-only admission for the line, and no
ordinary bypass, force-push, or deletion. Verify the first PR can supply the
required workflow; do not waive `Cargo` if bootstrap fails.

Bootstrap is a narrowly authorized administrative operation. Native rules must
restrict who may create protected lines where available; the operator verifies
the exact-stock starting SHA. A documented procedure is not proof that a rule
has been configured. Inspect the actual rules and account permissions.

For a mistaken existing line, preserve and verify archive refs before any
explicitly authorized reset. Limit a temporary protection exception to the named
line and operation, restore protections immediately, and verify the result.
Do not weaken historical accepted lines. This is corrective migration, not a
routine development step or authorization granted by these documents.

## One development loop and Green Prefix

1. Branch from the current version-line HEAD after its required post-merge proof
   is green; trusted exact stock is the initial input before C0 establishes CI.
2. Develop one understandable improvement. Test-first RED, fixups, and refactoring
   belong on the working branch, not canonical history.
3. Close the changed owner and its impact: source, owned outputs, delta tests,
   affected stock regressions, composition, and required proof activation together.
4. Open a PR to the version line. Review the obligation scope and implementation;
   require applicable proof and the current base required by native protection.
5. Squash the reviewed PR into one canonical development commit.
6. Run the same deterministic proof definition on that actual post-merge SHA.
   Do not advance to the next feature increment until it succeeds.

A canonical prefix is green when its exact canonical SHA has passed the shared
proof covering every still-applicable deterministic obligation introduced or
affected by that prefix, under the accepted admission mechanism. Green does not
require re-running unrelated upstream stock certification. It is relative to
reviewed obligations and stated assumptions, not universal product correctness.
A thin C0 can be green for mechanism obligations while product work remains undone.

PR proof authorizes admission of a candidate. Canonical proof verifies the object
actually admitted. Record PR HEAD, base, actual checkout/test SHA, and workflow
revision/context; do not equate a PR test-merge commit with its head or the final
squash SHA. GitHub's default `pull_request` subject is a merge context, as described
in its [event reference](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request).
The required result must apply to the current candidate and base. A changed
candidate or applicable base requires fresh applicable proof.

The canonical squash commits are the development history we retain; scratch
SHAs need not survive. Historical success is not current-subject evidence.
A successful successor does not turn a failed predecessor green.

If required canonical proof fails, is absent, is cancelled, or remains pending,
freeze further feature progression. Diagnose product, test, mechanism, or
infrastructure faults; repair/revert through a PR when needed. Do not erase
canonical history or relabel a failed run after narrowing scope. Any scope
correction is itself reviewed and receives fresh proof.

## Native CI, without a second test framework

Use one local reusable workflow, `.github/workflows/grok-proof.yml`, with
`workflow_call`. Keep native test/lint/generator invocations directly visible.
Do not add a Grok proof runner, semantic assertions in shell, or output interpreter.
A small native job-result aggregate is orchestration, not a product test.

The thin `.github/workflows/grok.yml` calls `./.github/workflows/grok-proof.yml`
for both PR and version-line push. Local reuse resolves from the caller's commit;
see [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_iduses).
Do not duplicate commands or call a moving external policy branch. The proof
definition is shared even though PR and canonical commit subjects differ.

`Cargo` is the stable required admission aggregate, not a promise to run the
entire stock Cargo suite. It may succeed only when every currently required proof
job succeeds. Review the explicit dependency graph at every aggregation layer:
missing, failed, cancelled, skipped, neutral, or empty required proof is not a
pass. Ensure the final aggregate is not itself silently skipped. GitHub's
[status-check rules](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)
do not by themselves enforce this stricter evidence contract. No aggregate can
recover an obligation silently deleted from the workflow; scope review remains
necessary. Source greps and empty test selections are not semantic proof.

Required event filters must not omit version-line PRs or canonical pushes.
Keep ordinary PR permissions minimal and backend secrets out. Do not solve
bootstrap by running untrusted candidate code in a privileged event context.
If no required run/check appears, report a bootstrap defect; absence is not green.

Scheduling may supersede outdated PR-head runs, but must not let a different
canonical SHA replace a pending or running required proof. Review caller/callee
concurrency namespaces and queue behavior, not only `cancel-in-progress`;
GitHub documents default pending replacement in its
[concurrency reference](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency).
Use subject-isolated canonical scheduling or another verified native arrangement
that preserves each required result. Cancellation remains non-green.

From C1 onward, add each increment's delta, affected-stock, and composition proof
to this same workflow while preserving coverage of earlier applicable obligations.
No owner-marker detection, owner/stage manifest, progressive/full mode, or
separate carry caller. Obligation retirement is not test selection by pipeline
phase. Composition is added when an interaction is introduced, not postponed to
final assembly; the last semantic owner closes the remaining semantic combination.

## Process capability and product acceptance

Harness mechanics and scenario source grow with their semantic owners. Carry
common support only when first used, with deterministic tests. Early Provider
proof must not depend on a later product catalog. Provider-neutral fixes do not
need invented Facts or Live scenarios for symmetry.

Process integration may wire backend Facts, distribution, manual diagnostic
Live, and artifact-to-Live orchestration. It owns deterministic tests for new
process behavior and its interactions, but cannot absorb forgotten semantic
proof or generated outputs from earlier owners.

Record `RECONSTRUCTED` only when planned product and process closure are supported:
selected owners and derived outputs are current; all still-applicable downstream
native, affected-stock, harness, and composition obligations pass; and selected
review, testing, Facts/Live invocation, and delivery capabilities exist at the
claimed level. An `UPDATE` label alone does not close a missing capability.
This is not a duplicate upstream stock certification exercise.

Record review acceptance against the actual green canonical SHA in the owning
GitHub roadmap/PR. No additional branch promotion or release state machine is
needed. Do not accept an incomplete line merely because some prefixes passed.
After acceptance, normal fixes use the same loop.

## Evidence boundaries

| Evidence | What it establishes | What it does not establish |
| --- | --- | --- |
| Trusted stock input | The agreed upstream/stock premise for exact `S`. | A downstream stock-CI execution or automatic correctness after modification. |
| C0 acceptance | The version contract and demonstrated admission/proof mechanism. | Stock or Grok product correctness. |
| Native tests, lint, consistency, deterministic harness tests | Scoped evidence for the tested downstream revision and its affected contracts. | Exhaustive correctness, current backend behavior, or product acceptance alone. |
| Facts | Backend observations with freshness and environment. | Product policy or catalog authority. |
| Real-provider Live | Observed end-to-end behavior for the selected runtime subject. | Native coverage or source acceptance alone. |
| Artifact smoke | Usability of the actual package tested. | Product/source authority. |

Live/Facts execution is parallel and non-blocking for initialization acceptance
unless an explicit product requirement needs a particular fresh claim.
Historical, not-rerun, and unknown observations stay labeled. Concrete discovered
defects still require triage; evidence separation does not excuse them.

Capability availability and execution are different. Preserve selected Facts/Live
and delivery mechanisms without pretending they ran. Actual distribution is
optional and governed by [distribution.md](./distribution.md).

## Execution, scope, and maintenance

For pending required CI, record the exact repository/run/attempt, revision/PR
subject, authoritative status source, completion condition, and next action.
Decide only from the settled result for that subject. Follow the current user's
execution/monitoring instructions; no particular bot is a policy prerequisite.
Missing permission or an unavailable entrypoint is a different external blocker.

Issue maps coordinate outcomes and real dependencies; they do not replace tests
or create a second policy. Reuse outcome owners. Keep scheduling order distinct
from technical prerequisites. Preserve historical comments/results; explicitly
supersede obsolete instructions rather than deleting evidence or calling it green.
Document publication does not implement workflows, satisfy C0, or close owners.

Keep `grok/main` a small process overlay on practical `openai/codex main`, with
only useful active experiments. Updating it does not reset version lines or
carry experiments into the product. Rebase/prune/history mutation requires its
own authorization. Preserve the overlay and classify discarded work.

Historical release/carry documents do not override current policy. Preserve
lessons, not accidental mechanisms. Do not add proof ledgers, source-promotion
bots, workflow-state models, or post-hoc PR provenance reconstruction. Review
through [review.md](./review.md); link policy rather than duplicating it.
