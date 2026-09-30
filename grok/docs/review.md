# Review one complete improvement

Use [Version-line-first development](./carry-forward.md) as policy. Review
whether another engineer could safely continue from this increment. CI green is
necessary evidence, not a substitute for that judgment.

## Establish the subject

Read the actual version-line HEAD, PR diff, exact target stock, accepted product
reference, and owning issue. Distinguish the line under construction from the
previous accepted product baseline. Inspect only the stock seams, tests,
generators, harnesses, and external evidence needed for this change.

Old carry results may explain why an approach works; they do not verify the new
PR. Do not infer completion from an issue title, earlier reviewer prose, a newer
branch name, or a green aggregate without inspecting its proof scope.

## Before merge

Check the following responsibilities once, at their owner:

- **Meaningful increment:** coherent behavior and explicit non-goals; no half API
  or knowingly broken dependency deferred to the next PR. Split by independently
  useful outcomes, not arbitrary file counts or historical patch boundaries.
- **Stock preservation:** use the pinned stock's architecture and native behavior.
  Keep provider-neutral correctness neutral; constrain Grok dialect changes to
  the explicit Provider boundary. Do not copy whole historical files over moved
  stock seams.
- **Owner closure:** source, relevant config/protocol representations, native
  generator outputs, build/test-data registration, and consistency tests travel
  together. No global generated-artifact gate and no terminal cleanup ticket.
- **Proof:** tests express behavioral assertions in the owning language. The PR
  adds applicable native commands to the existing shared reusable workflow;
  earlier obligations remain covered. Check selected tests actually run.
- **Harness:** deterministic helpers and scenario source accompany the behavior
  they test. Backend opt-in gates must not silently skip required native tests.
  Fixtures must not require product assets from a future increment.
- **Admission:** current required `Cargo` and review pass under native protections.
  Squash one increment; do not bypass a missing check, install custom fast-forward
  admission, or preserve scratch history merely for SHA identity.

Changing required test scope is itself reviewable. A renamed or removed test
needs an explicit semantic/stock-ownership justification, not an unexplained
coverage reduction. Cumulative compile success is not equivalent to running all
behavioral tests.

## After squash

Identify the actual canonical SHA and its push proof. It must invoke the same
native proof definition used by the PR, not a second command list or a stale
reusable-workflow revision. Do not start the next feature increment until the
required exact-head result is successful.

If that result fails, distinguish product, harness, orchestration, and external
infrastructure faults. Freeze feature progression and repair/revert through a
PR when needed. Preserve the failed result; do not rewrite the branch to invent
an uninterrupted green history. A corrected successor does not make the failed
predecessor green.

## Initial product acceptance

At the last initialization increment, check the version-specific plan against
the actual line:

- selected semantics and current-stock owner closure are complete;
- all accumulated native/harness and cross-owner composition proofs passed;
- selected admission, Facts/Live, and delivery capabilities are present and
  usable at the level claimed, not merely listed as UPDATE;
- the PR/push proof contract is shared and includes every required domain;
- evidence limitations, including backend work not run, remain explicit.

Record acceptance against an exact canonical green revision in the existing
roadmap or closing PR. Do not add an RC branch, final bulk promotion, or
`PR_PROVEN`/`SUCCESSFUL` product-state machine. Acceptance establishes the new
product baseline; normal development continues by the same PR/squash/push loop.

## Evidence and findings

Separate native proof, backend Facts, real-provider Live, and artifact smoke.
State whether evidence was checked, historical, not run, skipped, or unknown.
A workflow path existing does not prove it can execute; a passing run with every
backend test skipped is not Live proof. Inspect relevant invocation, dependency,
and credential requirements without requesting or disclosing secret values.

Report only concrete findings: owner, violated invariant, direct evidence,
consequence, and smallest correction. Distinguish a defect from an unverified
claim. Do not demand a redesign or reopen an owner solely because a hypothetical
risk can be imagined. Backend refresh remains parallel, but demonstrated defects
must be triaged on their merits.

For a pending CI gate, give the exact handoff defined in carry-forward.md and
pause for Grok Bot. Do not poll. For no material finding, report `none`.

## Context and authority discipline

Keep policy in the canonical document, executable assertions in native tests,
admission in GitHub protections, and dynamic results in GitHub. Link rather than
repeat. Do not build a second test interpreter, provenance database, or state
machine to re-evaluate GitHub's own state.

Review is read-only unless the user or invoking task authorizes the specific
mutation. Documentation or issue publication is not permission to reset branches,
change rulesets, implement product code, or publish packages.
