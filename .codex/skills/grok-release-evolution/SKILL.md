---
name: grok-release-evolution
description: Bootstrap and route Grok development-line carry-forward, PRE-CARRY contract extraction, progressive green reconstruction, version-line admission, runtime evidence, and optional distribution through the canonical grok/main doctrine. Do not use for ordinary stock Codex work.
---

# Grok Development-Line Evolution

This skill is only the bootstrap/routing entrypoint.

The canonical process authority is:

- `grok/main:grok/docs/carry-forward.md`

Read distribution policy only when packaging or human-use artifacts are relevant:

- `grok/main:grok/docs/distribution.md`

Read review policy when auditing a carry or accepted line:

- `grok/main:grok/docs/review.md`

Do not fork those rules into this skill.

## North Star

Carry-forward moves a **living software-development line** onto new stock.

Do not treat the task as:

- replaying old commits;
- applying a patch series;
- copying only product semantics;
- producing one final green snapshot.

The result must be able to continue normal development, proof, admission,
runtime validation, and optional distribution on the new stock generation.

## Bootstrap

Before product reconstruction:

1. Read the current canonical doctrine from `grok/main`.
2. Resolve the newest accepted `grok/rust-v*` source SHA.
3. Resolve the exact target stock tag/SHA.
4. Resolve any verified semantics-preserving continuation rewrite.
5. Enter PRE-CARRY before editing product semantics.
6. Do not bootstrap from a carry branch's local process files; carry workspaces
   intentionally start from exact stock.

Workflow runs, artifacts, releases, Facts, and Live do not create source
authority.

## PRE-CARRY

Extract a development contract from the accepted line and target stock.

For each relevant item record:

- semantic behavior or development capability;
- owner;
- dependencies;
- deterministic proof;
- Facts/Live evidence when relevant;
- admission/release role;
- `KEEP`, `UPDATE`, or `DROP` decision.

Run/verify:

- previous accepted behavior characterization where needed;
- exact target-stock native baseline.

Check whether target stock now owns each behavior before reconstructing
downstream mechanisms.

Plan the new stack by current semantic dependency, not historical commit order.

## TDD

Use TDD freely while developing:

~~~text
test/characterization -> RED
implementation -> GREEN
refactor -> GREEN
~~~

Do not publish canonical carry commits that intentionally leave the branch
broken.

Fold temporary RED development steps into the closed green semantic increment
they establish.

## Carry bootstrap

Start `carry/grok-rust-vX.Y.Z` from exact target stock.

Establish the minimum progressive deterministic proof machinery while pure
target stock is still green.

Prefer one shared proof implementation used by:

- progressive carry validation;
- final carry validation;
- version-line PR admission.

Do not maintain separate command lists that can drift.

## Progressive development

Treat every semantic commit as a closed vertical development increment.

It should carry, as applicable:

- intent;
- source implementation;
- owned derived outputs;
- native tests;
- deterministic harness pieces;
- proof activation.

After every published increment, run the accumulated deterministic proof for
that prefix.

Previously active proof must remain active.

Never advance the canonical stack while its current prefix is red.

Respect semantic dependencies between owners.

## RECONSTRUCTED

Call the candidate `RECONSTRUCTED` only when both are closed:

### Product closure

- selected semantics classified;
- retained/updated owners implemented;
- owned outputs current;
- native proof green;
- cross-owner composition green.

### Development-process closure

- progressive proof complete;
- final carry proof equivalent to version-line PR deterministic proof;
- inherited admission/Facts/Live/release capabilities preserved, reconstructed,
  or explicitly dropped;
- the new line can continue normal development after admission.

Real-provider Live and actual artifact distribution need not have run.

## Final admission checks

Before version-line PR:

1. Verify final deterministic proof equivalence:
   `CarryFinalProof == VersionLinePRProof`.
2. Perform the short capability-continuity guardrail.
3. Classify inherited capabilities as `KEEP`, `UPDATE`, or `DROP`.
4. Separate admission-required, parallel-evidence, and distribution-only roles.

If the version-line PR exposes new deterministic proof that final carry did not
run, fix the proof architecture rather than accepting divergent CI authorities.

## Version-line bootstrap and admission

A version-line branch name is not source authority.

The first authoritative `grok/rust-vX.Y.Z` head must have an explicit
relationship to required version-line PR proof and enter through the native
protected-branch path.

If GitHub mechanics require a target ref to exist first, treat that ref as
provisional until a non-empty PR-proven transition is admitted.

After native PR proof and protected-branch admission, the resulting head is
accepted source.

Do not substitute branch creation, build success, Live, or artifact availability
for admission.

## Facts and Live

Keep environment-dependent evidence separate from deterministic proof.

Facts:

- backend observations;
- may inform product decisions;
- never source authority.

Live:

- runtime/backend evidence;
- carry its harness capability when part of the accepted development process;
- do not require real-provider execution for every semantic prefix.

When Live consumes a packaged artifact, classify whether it is product/runtime
evidence, artifact/release evidence, or both.

## Distribution

Actual distribution is optional.

When users need downloadable artifacts:

1. read `grok/main:grok/docs/distribution.md`;
2. start from an explicitly selected exact source SHA;
3. normally use an accepted version-line head;
4. build each requested target once;
5. stage the complete package;
6. run package-level smoke only where useful;
7. preserve exact source identity in artifact metadata.

A failed artifact build is artifact unavailability unless direct evidence shows
a product defect.

## grok/main maintenance

`grok/main` carries current process doctrine plus optional active experiments.

Keep the process overlay small, current, and easy to rebase.

Prune finished experiments. Prefer preserving reusable development invariants
over historical implementation patches.

A process lesson discovered during carry should be expressed as a current
invariant when it will prevent future classes of failure.

## Historical diagnosis

Use older version lines only when current doctrine, latest accepted state,
target stock, and direct evidence cannot answer a concrete question.

Stop once intent/ownership is understood.

Historical implementation order is evidence, not the plan for the next carry.

## Mechanism discipline

Prefer:

1. native owner tests;
2. shared deterministic proof commands;
3. thin carry/version-line wrappers;
4. native GitHub review/rulesets;
5. separate Facts/Live;
6. simple optional distribution.

Avoid custom proof ledgers, workflow-state machines, post-hoc provenance
reconstruction, and duplicate CI authorities.

## Authorization

Review findings are not write authorization.

Do not mutate authoritative branches, merge PRs, publish artifacts, or rewrite
history unless the user or invoking task authorized the action.

After a mutation, verify the exact ref or artifact effect owned by that action.
