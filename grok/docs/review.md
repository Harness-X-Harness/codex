# Grok development-line review

This document is the canonical current review policy for Grok evolution.

Review the health of the **development line**, not only its final source diff.

The current evolution doctrine is
[grok/docs/carry-forward.md](./carry-forward.md). Distribution policy is
[grok/docs/distribution.md](./distribution.md).

## Review objective

A successful carry must leave a line that can continue normal software
development on the new stock generation.

Review therefore asks two independent questions:

~~~text
Is the product behavior reconstructed correctly?

Can the reconstructed line still develop, prove, admit, validate, and
optionally distribute that product normally?
~~~

A green final snapshot is insufficient if intermediate development increments
were broken, proof disappeared during the stack, admission uses a different
proof model, or accepted validation capability vanished accidentally.

## Authorities

Use:

- current `grok/main` doctrine for process;
- newest accepted `grok/rust-v*` source for accepted product state;
- that accepted line's tests/workflows/harness/contracts as evidence of the
  development process to classify;
- exact target stock tag/SHA for target architecture and native behavior;
- explicit product decisions for intentional change.

Do not infer source authority from artifacts, release pages, workflow runs,
Facts, or Live.

## Review order

Review in this order:

1. PRE-CARRY contract and baseline selection;
2. carry bootstrap;
3. each published development increment;
4. proof accumulation and dependency order;
5. final composition;
6. final carry/version-line proof equivalence;
7. capability continuity;
8. first-head/version-line admission;
9. parallel Facts/Live evidence;
10. optional distribution.

Stop at the first correctness boundary that makes later conclusions invalid.
Do not manufacture findings for already-closed surfaces.

## PRE-CARRY review

Verify that PRE-CARRY identified:

- accepted source baseline;
- exact target stock;
- selected product decisions;
- relevant semantic owners and dependencies;
- existing deterministic characterization/proof;
- inherited development-process capabilities;
- `KEEP`, `UPDATE`, or `DROP` classification with rationale.

Check that exact stock passes its required native baseline before Grok
reconstruction begins.

Report a PRE-CARRY defect when the work starts from an artifact-producing SHA
instead of accepted source, silently assumes old implementation shape, lacks a
meaningful contract for retained behavior, or ignores stock ownership.

Finding prefix: `PC-`.

## Green Prefix review

Every published prefix of the canonical semantic stack must pass the
deterministic proof applicable to that prefix.

Look for:

- commits that intentionally leave compile/test failures for later repair;
- generated outputs fixed only by a later unrelated commit;
- owner tests introduced long after the behavior they prove;
- proof temporarily disabled to permit an intermediate state;
- dependency owners appearing in reverse order.

A local TDD RED step is not a defect if it was folded before publication into a
closed green development increment.

Finding prefix: `GP-`.

## Closed Development Increment review

For every semantic commit, verify that it is a meaningful vertical development
increment.

Expected closure:

~~~text
intent
-> current stock seam
-> implementation
-> owned derived outputs
-> native proof
-> deterministic harness/proof activation
~~~

Historical patch decomposition is not a valid reason to split one semantic
behavior into multiple broken commits.

Conversely, do not require one giant commit when a large owner has independently
meaningful, independently green sub-increments.

Finding prefix: `DI-`.

## Proof Monotonicity review

Proof established by an earlier prefix must remain active as later increments
are added.

Review the accumulated proof surface, not only the newest job.

Report when:

- an earlier owner proof becomes skipped or unreachable;
- later CI narrows coverage without explicit rationale;
- one owner can shadow or replace another owner's required proof;
- the composition job can succeed without all applicable owner proofs.

Finding prefix: `PM-`.

## Owner and derived-output closure

For each changed owner verify:

~~~text
changed behavior
-> source-of-truth seam
-> owned checked-in/runtime outputs
-> stock generator where applicable
-> native consistency/regression proof
~~~

Do not require a repository-wide generated-file audit. Follow changed ownership.

Report stale generated/precomputed/runtime representations or a missing
owner-level proof.

Finding prefix: `OC-`.

## Dependency and composition review

Verify that semantic dependency order reflects current architecture rather than
historical commit order.

A dependent owner must not become active before its prerequisite.

At final carry, require:

- every planned retained/updated owner proof;
- cross-owner compile/composition proof;
- no hidden reliance on a later release workflow to discover deterministic
  integration failures.

Finding prefix: `CO-`.

## Final Proof Equivalence review

This is a primary carry invariant.

Verify:

~~~text
CarryFinalProof(candidate)
    == VersionLinePRProof(candidate)
~~~

The wrappers may differ, but the deterministic proof domains must be equivalent.

Report when version-line PR admission introduces deterministic tests,
consistency checks, harness unit tests, lint, or composition proof that final
carry never ran.

Also report duplicated carry/version-line command lists that have already
drifted or can trivially drift.

Prefer shared proof implementation with thin wrappers.

Finding prefix: `FE-`.

## Capability Continuity review

Treat this as an anti-omission guardrail after progressive reconstruction.

Compare the latest accepted line with the candidate for development-process
capabilities, not file identity.

At minimum classify:

- required PR proof / public aggregate checks;
- branch/ruleset admission;
- Facts/backend-observation capability;
- Live/runtime harness and invocation;
- manual diagnostic entrypoints;
- artifact-to-Live relationships;
- release/distribution capability;
- external prerequisites required to make those capabilities usable.

Each item must be `KEEP`, `UPDATE`, or `DROP` with rationale.

Classify its role:

- admission-required;
- parallel evidence;
- distribution-only.

A missing capability is a defect when it disappeared implicitly.

Finding prefix: `CC-`.

## RECONSTRUCTED review

`RECONSTRUCTED` requires both:

### Product closure

- selected semantics classified;
- retained/updated owners closed;
- owned derived outputs current;
- native proofs green;
- composition green.

### Development-process closure

- progressive deterministic proof complete;
- final version-line deterministic proof available and equivalent;
- inherited validation/admission/evidence/release capabilities preserved,
  reconstructed, or explicitly dropped;
- the line can continue normal development after admission.

Real-provider Live and actual distribution need not have run.

Finding prefix: `RC-`.

## First-head and version-line admission review

Creating a `grok/rust-vX.Y.Z` ref is not admission proof.

Verify that the first authoritative head has an explicit relationship to the
required version-line PR proof and enters through the native protected-branch
path.

If a target ref had to be created first for platform mechanics, treat it as
provisional until a non-empty PR-proven transition is admitted.

Report direct branch creation, build success, Live success, or artifact
availability being used as a substitute for required PR admission.

Finding prefix: `VA-`.

## Evidence-class review

Keep these classes distinct.

### Deterministic implementation proof

Native tests, lint, generated consistency, deterministic harness unit tests, and
composition.

This composes progressively and participates in final proof equivalence.

### Facts

Backend observations. Useful for product decisions and evidence classification,
not source authority.

### Real-provider Live

Runtime/backend evidence. Its capability may need to carry; its execution is not
required for every semantic prefix.

### Distribution smoke

Evidence that a packaged artifact is usable. It is derivative of selected
source.

Report any workflow or document that blurs these authority boundaries.

Finding prefix: `EC-`.

## Distribution review

Review distribution separately under
[grok/docs/distribution.md](./distribution.md).

Check:

- exact source SHA identity;
- requested target set;
- package completeness;
- artifact-level smoke actually consuming the package;
- human installation usability.

A failed or absent artifact is artifact unavailability, not semantic rejection.

Finding prefix: `DR-`.

## Mechanism review

Prefer the smallest mechanism that protects an independent invariant.

Good default order:

1. native owner tests;
2. shared deterministic proof commands;
3. thin carry/version-line wrappers;
4. native GitHub review/rulesets;
5. separate environment-dependent Facts/Live;
6. simple distribution.

Flag:

- proof ledgers;
- custom workflow-state machines;
- historical PR/run provenance reconstruction;
- duplicate authorities;
- helpers that merely mirror GitHub state;
- multiple CI implementations for the same deterministic proof.

Finding prefix: `MS-`.

## Historical diagnosis

Use older version lines only when current doctrine, latest accepted line, target
stock, and direct evidence cannot resolve a concrete question.

Historical commit order is evidence, not a reconstruction plan.

Stop once ownership, behavior, or capability intent is understood.

## Review output

For every finding report:

- finding ID;
- owner/invariant;
- direct evidence;
- smallest correction;
- correctness boundary;
- risk if left unresolved.

For a carry candidate also report:

- accepted baseline;
- exact target stock;
- current published-prefix health;
- first red prefix, if any;
- product closure;
- development-process closure;
- final proof equivalence;
- capability continuity;
- whether `RECONSTRUCTED` is justified;
- first unsatisfied admission requirement.

Report Facts/Live/distribution status separately.

If there is no meaningful finding, report `none`.

## Mutation boundary

Review is read-only by default.

Do not mutate code, docs, issues, PRs, workflows, branches, artifacts, or
releases without explicit authorization from the user or invoking task.
