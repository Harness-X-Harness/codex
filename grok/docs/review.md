# Review one complete improvement

Use [Version-line-first development](./carry-forward.md) as policy. Review
whether another engineer could safely continue from this increment. CI green is
necessary evidence, not a substitute for judgment about scope and coverage.

## Establish the subject and assumptions

Read the actual version-line HEAD, PR diff, exact target stock, accepted product
reference, and owning issue. Distinguish trusted stock input, a development line,
and an accepted Grok product baseline. Verify the identity of `S`; do not make
re-certification of untouched stock an implicit review requirement.

Inspect the affected stock seams, callers, shared dependencies, configuration,
generators, harnesses, and runtime boundaries. Unchanged files can still be
impacted. Neither a narrow diff nor a convenient test filter establishes the
impact boundary. Investigate uncertainty or broaden relevant proof.

Old carry results may explain an approach; they do not verify a new PR. Do not
infer completion from an issue title, earlier reviewer prose, branch naming, or
a green aggregate without inspecting its scope and actual execution subject.

## Before merge

Check these responsibilities at their owner:

- **Meaningful increment:** coherent behavior and explicit non-goals; no half API
  or knowingly broken dependency deferred to another PR. Split by independently
  useful outcomes, not arbitrary file counts or historical patch boundaries.
- **Stock preservation:** use current stock architecture; keep provider-neutral
  correctness neutral and Grok dialect changes at the explicit Provider boundary.
  Do not copy historical files over moved seams or assume trust in `S` proves `S + delta`.
- **Owner closure:** source, affected config/protocol representations, native
  generated outputs, build/test-data registration, and consistency checks travel
  together. No global generated-file gate or terminal cleanup ticket.
- **Obligation coverage:** identify introduced behavior, affected stock contracts,
  and interactions with earlier increments. Native assertions and environments
  must cover those responsibilities in the shared reusable proof. Check meaningful,
  non-empty execution and preservation of earlier applicable obligations.
- **Harness:** deterministic helpers and scenario source accompany their behavior.
  Backend opt-ins must not skip required native tests. Fixtures cannot depend on
  product assets from a future increment.
- **Admission:** current required `Cargo` and review pass under native protection.
  Verify the result's applicability to the current head/base and actual checkout.
  Squash one increment; do not bypass missing checks or add custom admission.

Proof scope is itself reviewed. Replacing a test requires equivalent or stronger
coverage of its continuing obligation. Retiring an obligation requires an explicit
semantic/scope decision, not a failing run or a desire to shorten CI. Keep the
rationale in the PR and update the version plan when its boundary changes.
Executed once does not mean owned forever; inherited obligations also cannot
silently disappear. A broader suite may be appropriate for a shared seam.
Compile success is not equivalent to behavioral test execution.

## C0 review: mechanism acceptance, not stock certification

Review C0 as a complete infrastructure increment: version contract, native
admission/proof path, and proof architecture bootstrap. It may contain a thin
subject-aware bootstrap job without workspace Cargo tests or Clippy. Reject
stock fixture, version, lockfile, or product repairs whose purpose is to turn
stock baseline CI green; independently justified stock adaptations get their
own scoped increment.

Require structural review and dynamic evidence, not merely valid YAML:

- exact-stock provenance, explicit trust boundary, and no future-owner placeholders;
- real PR invocation of the local reusable proof and actual enforcement of the
  expected required `Cargo`, including its source and applicability;
- subject identity, permissions, event coverage, explicit required dependencies,
  and caller/callee concurrency that preserves canonical evidence;
- an applicable executed negative path plus review/focused checks for missing,
  failed, skipped, cancelled, neutral, and empty required proof; the final aggregate
  must not silently skip or convert non-success to success;
- fresh successful final-candidate PR proof and separate successful push proof
  on the actual canonical squash SHA using the same definition.

The mechanism acceptance record can support unchanged invariants later; modifying
a relevant workflow, filter, permission, subject mapping, or aggregate requires
corresponding revalidation. A previous failed run can establish a narrowly
applicable failure-path observation, not success for a revised candidate.
C0 closure is neither stock correctness nor Grok product acceptance.

## After squash

Identify the actual canonical SHA, its checkout/test subject, workflow revision,
and required push result. It must use the same deterministic proof definition
as the PR, not another command list or a stale/moving external revision.
Distinguish PR HEAD, test-merge SHA, and canonical SHA. Do not start the next
feature increment until required exact-head proof succeeds.

A green canonical prefix covers all still-applicable deterministic obligations
introduced or affected by that prefix. It does not assert unrelated upstream CI
was rerun, nor does it establish exhaustive correctness.

For failed, absent, cancelled, or pending required proof, freeze progression.
Classify product, harness, orchestration, and infrastructure faults. Repair or
revert through a PR when needed; preserve the original result. A corrected
successor or reviewed scope change does not turn the predecessor green.

## Initial product acceptance

Check the version-specific plan against the actual line:

- selected semantics and current-stock owner closure are complete;
- every still-applicable downstream native, affected-stock, harness, and cross-owner
  composition obligation is covered and has passed on the actual canonical SHA;
- selected admission, Facts/Live, and delivery capabilities exist and are usable
  at the claimed level, not merely labeled UPDATE;
- PR/push share the required proof definition; no semantic proof or generated
  closure first appears during final package/process assembly;
- upstream trust assumptions, unexecuted backend work, and evidence limitations
  remain explicit. Final review is not a stock-wide re-certification exercise.

Record acceptance against an exact green canonical revision in the existing
roadmap or closing PR. Do not add an RC branch, bulk promotion, or a
`PR_PROVEN`/`SUCCESSFUL` product-state machine. Normal development then continues
through the same PR/squash/push loop.

## Evidence and findings

Separate trusted stock, C0 mechanism acceptance, downstream deterministic proof,
backend Facts, real-provider Live, and artifact smoke. Label evidence as checked,
historical, not run, skipped, or unknown. Workflow existence does not establish
invocability; backend skips are not Live passes. Check permissions and invocation
requirements without requesting or disclosing secret values.

Report concrete findings: owner, violated invariant, evidence, consequence, and
smallest correction. A failure outside downstream impact is not automatically a
C0 blocker. A demonstrated defect in a premise we rely on must still be triaged.
Do not demand universal redesign from a hypothetical risk, or dismiss a relevant
stock regression because the original stock was trusted.

For pending CI, record the exact subject, authoritative source, and next action;
follow current user instructions rather than requiring a particular monitor.
For no material finding, report `none`.

## Context and authority discipline

Keep policy in the canonical document, semantic assertions in native tests,
admission in GitHub protections, and dynamic evidence in GitHub. Link rather
than repeat. Do not build another test interpreter, provenance database, or
state machine to re-evaluate GitHub's own state.

Review is read-only unless the task authorizes mutation. Documentation or issue
publication does not implement C0, complete owners, authorize branch resets or
ruleset changes, or publish packages. Historical comments remain evidence even
when their old instructions are explicitly superseded.
