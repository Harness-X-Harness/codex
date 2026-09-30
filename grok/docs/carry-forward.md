# Grok product evolution

This document is the canonical current authority for Grok product evolution.
The repo-local skill is only the bootstrap and routing entrypoint. It must not
fork these rules.

Product evolution and binary distribution are separate concerns.

- Product evolution decides which source semantics continue onto new stock.
- Distribution turns an accepted source SHA into artifacts that humans can
  download.
- Distribution success, failure, or absence does not grant, revoke, or delay
  semantic authority.

Current distribution policy lives in [distribution.md](./distribution.md).

## Authorities

~~~text
grok/main:grok/docs/carry-forward.md
    = current product-evolution policy

latest accepted grok/rust-v*
    = authoritative accepted product state for that stock generation

verified semantics-preserving baseline rewrite, when one exists
    = optional continuation representation derived from an accepted state

exact target stock rust-vX.Y.Z
    = target harness and stock-behavior authority

explicit product decisions since the previous accepted state
    = intentional semantic changes

relevant validated grok/main experiments
    = optional candidate inputs

older accepted grok/rust-v*
    = historical evidence when diagnosis is needed

carry/grok-rust-vX.Y.Z
    = temporary reconstruction and review workspace

grok/main:grok/docs/distribution.md
    = current binary-distribution policy
~~~

Do not infer semantic authority from a build, artifact, GitHub Actions run,
release page, or download channel.

The carry workspace is the work object, not the process bootstrap source.
Start product evolution from the `grok/main` entrypoint so the current doctrine
is read even when the exact stock tag and carry branch intentionally do not
contain the process overlay.

## Baseline resolution

The normal starting point is the latest accepted version-line source. Before
carrying it forward, resolve the best semantic baseline for continuation.

Use the accepted source directly when its history and tree are already a clean
representation of the product semantics.

A semantics-preserving rewrite may instead be used when the accepted source
contains accidental representation noise, historical commit decomposition,
generated churn, versioning churn, or another artifact that should not be
propagated into the next reconstruction.

A baseline rewrite must satisfy all of these rules:

1. Name the accepted source SHA it is derived from.
2. Classify every difference from that source.
3. Preserve the accepted product semantics.
4. Close any changed owner whose runtime-consumed or checked-in representation
   is affected.
5. Keep intentional behavior changes out of the rewrite; those are explicit
   product decisions for the next reconstruction.
6. Remain disposable. It is a continuation aid, not another long-lived product
   trunk.

A baseline rewrite does not need binary distribution. Distribution does not
establish semantic equivalence or source authority.

Baseline resolution is a transition, not a durable product state.

## Product evolution path

The normal evolution path is:

~~~text
accepted semantic state
        |
        +-- optional verified semantics-preserving baseline rewrite
        |
        + explicit product decisions
        |
        + selected useful experiments, if any
        |
        + exact target stock
        v
semantic classification and reconstruction
        v
owner + derived-output closure
        v
RECONSTRUCTED
        v
required semantic validation and review
        v
version-line capability parity review
        v
admission to grok/rust-vX.Y.Z
        v
accepted version-line source
        |
        +--> default input for future baseline resolution
        |
        +--> optional binary distribution
~~~

The goal is not to reproduce the previous implementation or its exact history.
Preserve or intentionally change product semantics on the new stock
architecture.

For every candidate behavior ask:

~~~text
Does target stock now own it?
    yes -> use stock and drop the downstream mechanism

Does the product still require it?
    yes -> reconstruct the smallest semantic at the current stock seam

Was the behavior intentionally changed since the previous accepted state?
    yes -> implement the new product decision

Is it only a grok/main experiment?
    yes -> omit unless explicitly accepted

Is it an obsolete workaround or historical intermediate state?
    yes -> drop

Is it provider-neutral correctness behavior?
    -> check target stock first; retain only when still needed
~~~

No-path-conflict is not semantic proof. Nearby upstream churn is not evidence
that Grok needs a patch. A green artifact build is also not semantic proof.

## RECONSTRUCTED

`RECONSTRUCTED` is the carry milestone at which:

- selected product semantics are implemented on the exact target stock;
- each changed semantic owner is closed;
- every owned checked-in or runtime-consumed derived output is current;
- the candidate is ready for semantic validation and review.

`RECONSTRUCTED` does not mean that binaries have been built or distributed.
Those operations are optional derivatives of an accepted source.

Do not add product states merely to mirror GitHub workflow phases.

## Semantic closure

For every retained or intentionally changed semantic, close only the owner that
changed:

~~~text
changed semantic owner
    -> source-of-truth seam
    -> that owner's checked-in or runtime-consumed derived outputs
    -> that owner's native consistency/regression proof
~~~

Use the target stock tree to discover ownership. Do not keep a global generated
file checklist.

When the owner has a stock generator, run it instead of editing its outputs
independently. Include the resulting owned outputs and run the stock consistency
tests. `RECONSTRUCTED` is not reached while an owned derived representation is
stale.

Do not search the repository for unrelated caches, locks, schemas, SDKs, or
serialized forms merely because they are generated. Closure follows the changed
owner.

If the target owner lacks a generator or consistency test that is necessary for
correctness, add the smallest owner-level proof. Do not create a parallel
downstream generation framework.

## Carry workspace

A matching `carry/grok-rust-vX.Y.Z` branch is temporary reconstruction and
review work. It starts from the exact target stock tag or exact target stock
commit and reconstructs the selected semantic set.

Do not make it another long-lived product authority.

Construct commits by current semantic owner. Fold historical probes, fixes,
schema refreshes, generated cleanup, and commit surgery when they now describe
one settled behavior. Provider-neutral fixes remain provider-neutral.

Before version-line admission, verify each changed owner is closed and call the
candidate `RECONSTRUCTED`. Then complete the version-line capability-parity
review so validation/release capabilities inherited from the previous accepted
line are either preserved or intentionally reclassified.

## Version-line capability parity

Before admitting a reconstructed candidate to `grok/rust-vX.Y.Z`, compare the
candidate with the latest accepted Grok version line for **validation and release
capabilities**, not implementation shape.

Classify each relevant version-line capability as `KEEP`, `UPDATE`, or
`DROP`, with a short rationale. At minimum review:

- required PR checks and their proof domains;
- branch protections or rulesets that define admission;
- manual `workflow_dispatch` entrypoints;
- Facts/backend-observation workflows;
- Live/runtime-validation workflows;
- artifact-to-Live validation paths;
- release/distribution workflow ownership;
- secrets, artifact dependencies, and other external prerequisites that make
  those workflows usable.

This is a capability-parity gate, not a requirement to replay old workflow
files verbatim. Reconstruct the capability at the current repository seam when
stock or workflow architecture changed.

The gate must distinguish three roles:

~~~text
admission-required capability
    = must exist and be usable before version-line admission

parallel evidence capability
    = should remain available, but its execution does not define source authority

distribution-only capability
    = belongs to optional artifact/release work and does not block semantic continuity
~~~

Live and distribution remain separate from source authority. A Live run does
not need to succeed before `RECONSTRUCTED`, and a distribution artifact does
not make a source SHA canonical. However, silently dropping an accepted
version-line validation capability is not allowed. A missing prior capability
must have an explicit `UPDATE` or `DROP` decision before admission.

Creating a branch named `grok/rust-vX.Y.Z` does not itself complete admission.
Admission is complete only after semantic review, owner closure, required native
checks, and this version-line capability-parity review are all satisfied.

## Semantic validation and version-line admission

The version line `grok/rust-vX.Y.Z` records the accepted Grok source state for
that stock generation.

Admission should use the repository's native review, required semantic checks,
and branch protections. Those mechanisms protect the accepted source; they do
not derive authority from binary distribution.

Required checks should prove the semantics and owner closure that the candidate
actually changes. Keep the public check surface stable where useful, but do not
turn workflow phases into product states.

Once a candidate is accepted and admitted to the version line, it may seed
future baseline resolution immediately. Artifact availability is irrelevant to
that semantic continuity.

A later semantics-preserving history or tree rewrite may be used as the
continuation baseline without pretending that the rewrite was itself
distributed. Record the accepted source it derives from and the verified diff
classification.

If a rewrite changes product behavior, it is not baseline normalization. Treat
the change as a new product decision and validate it normally.

## Distribution boundary

Binary distribution is optional and independent. See
[distribution.md](./distribution.md).

The direction of authority is one-way:

~~~text
accepted source SHA
        v
distribution build
        v
downloadable artifacts
~~~

Never invert that relationship. Artifact success does not make source
semantics canonical, and artifact failure does not make accepted source
semantics non-canonical.

A Live test may serve one of two roles:

- semantic acceptance evidence, when it verifies a product behavior and is
  owned by the product validation contract;
- distribution smoke evidence, when it verifies that a packaged artifact is
  runnable and composed correctly.

Do not let a workflow implementation blur those roles.

Facts are backend observations. They may inform semantic decisions but do not
gain authority from distribution.

## Mechanism boundary

Keep process infrastructure smaller than the product it supports.

Prefer, in order:

1. native owner tests for product semantics;
2. native repository review/checks for version-line admission;
3. minimal GitHub branch protection as a preventive guardrail;
4. simple distribution jobs only when downloadable artifacts are wanted.

Do not add a second proof authority merely to test or reinterpret GitHub's own
state. Avoid proof ledgers, persistent synchronization state, publisher state
machines, custom workflow-state models, and helper frameworks whose only job is
to mirror a few GitHub API predicates.

Extract decision logic into code only when it has independent stable semantics
or meaningful reuse beyond making one workflow testable.

## Historical version lines

The newest accepted version line is the normal source for baseline resolution.

Older accepted lines are useful only when the latest accepted state, target
stock, and current doctrine do not explain a semantic cleanly. They are normally
frozen.

Use older history to understand why a semantic existed or where ownership
moved. Do not replay historical implementation sequences.

Historical `release.md` or older carry-forward documents describe how those
lines were operated at the time. They are not current product-evolution policy.

Inspect the nearest useful history first and stop once the semantic is
understood.

## grok/main

`grok/main` is deliberately cheap to rebase and safe to prune.

Its normal shape is:

~~~text
current practical openai/codex main
+
one current Grok process overlay
+
optional short-lived experiment commits
~~~

Normal maintenance is to periodically rebase `grok/main` onto current stock
upstream main. The authority is `openai/codex main`, not a local remote name.

Prefer rebase to merging stock main into `grok/main`.

The process overlay is not an experiment. It is the minimum valid final state
of `grok/main` and survives routine pruning.

Use these terms consistently:

~~~text
rebase main
    = move the workspace to newer practical stock main while preserving the
      process overlay and useful experiments

prune experiments
    = remove finished, stale, or superseded experiment commits while
      preserving the process overlay

rebuild main
    = reconstruct the single current process-overlay commit on stock main

hard reset to stock
    = temporary mechanical step during rebuild only; pure stock is not the
      normal final state of grok/main
~~~

When there are no active experiments, keep the process overlay as one commit
where practical. Rebase useful experiment intent to current seams; prune stale
experiments.

Rebase, prune, or rebuild `grok/main` does not change any version line and is
not carry-forward, backport, or product promotion.

## Feedback

Carry work may produce useful lessons. A lesson can later be re-expressed on
`grok/main`, but this is optional and does not determine continuity of the next
version line.

~~~text
preserve the lesson
not the patch
~~~

Re-evaluate it against current stock main and use the current seam.

## Backports

Do not routinely modify historical accepted version lines. Backport only when a
concrete support, correctness, security, or explicitly requested distribution
requirement exists.
