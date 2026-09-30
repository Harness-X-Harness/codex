# Grok development-line evolution

This document is the canonical current authority for Grok carry-forward.

## North Star

Carry-forward does not copy a patch, a commit sequence, or only a set of product
semantics. It moves a **living software-development line** onto new stock.

A successful carry leaves the new line able to continue normal development:

- product behavior is reconstructed at current stock seams;
- each development increment has deterministic proof;
- proof composes as the stack grows;
- review and version-line admission are usable;
- inherited Facts and Live evidence capabilities are preserved or intentionally
  changed;
- release/distribution capability is preserved or intentionally changed when it
  is part of the accepted development process.

The accepted source SHA remains product authority. Workflow runs, Live results,
artifacts, and download channels are evidence or derivatives; they do not create
source authority.

~~~text
we do not carry a patch
we do not carry only semantics
we carry a living development line onto new stock
~~~

## Authorities

~~~text
grok/main:grok/docs/carry-forward.md
    = current development-line evolution policy

latest accepted grok/rust-v*
    = authoritative accepted source state for that stock generation

that accepted line's tests, workflows, harnesses, and branch-local contracts
    = evidence of the development process that must be classified for continuity

verified semantics-preserving baseline rewrite, when one exists
    = optional continuation representation derived from accepted source

exact target stock rust-vX.Y.Z
    = target architecture, harness, stock behavior, generators, and native-test authority

explicit product decisions since the previous accepted line
    = intentional behavior changes

validated grok/main experiments
    = optional candidate inputs

grok/main:grok/docs/distribution.md
    = current distribution policy
~~~

The carry workspace is temporary. Start the process from the current
`grok/main` doctrine, even when the exact stock tag and carry branch
intentionally do not contain the process overlay.

## Permanent invariants

### 1. Green Prefix

Every published prefix of the canonical carry stack must pass the deterministic
proof applicable to that prefix.

If commits are `C1 ... Cn`, then every state below is a supported development
milestone:

~~~text
stock + C1
stock + C1 + C2
...
stock + C1 + ... + Cn
~~~

Do not preserve canonical commits whose only purpose is to leave the branch
temporarily broken until a later fix.

### 2. Closed Development Increment

A semantic commit is a closed vertical development increment, not merely a
source-code patch.

It carries together the parts necessary to establish one meaningful product
change at that stage:

~~~text
intent
+ implementation at the current owner seam
+ owned derived outputs
+ native regression/consistency proof
+ deterministic harness pieces owned by that behavior
+ proof activation needed for the new state
~~~

If an owner is too large for one increment, split it only at independently
meaningful, independently green development boundaries.

### 3. Monotonic Proof

As the carry stack grows, proof grows with it. Adding a new development
increment may add proof, but must not silently disable proof already established
by an earlier prefix.

Conceptually:

~~~text
Proof(C1) ⊆ Proof(C1+C2) ⊆ ... ⊆ Proof(C1+...+Cn)
~~~

### 4. Final Proof Equivalence

The completed carry candidate and the version-line PR must use the same
deterministic product-proof contract.

~~~text
CarryFinalProof(candidate)
    == VersionLinePRProof(candidate)
~~~

Workflow wrappers may differ, but a version-line PR must not discover an
entirely new deterministic proof domain that the completed carry never ran.

Release builds, real-backend Live execution, and optional distribution are not
part of this equality.

### 5. Capability Continuity

An accepted development-line capability may be `KEEP`, `UPDATE`, or
`DROP`, but it must never disappear implicitly.

This applies to semantics and to development-process capabilities such as
required PR proof, admission controls, Facts, Live harnesses, manual diagnostic
entrypoints, release composition, and distribution machinery.

## Evolution model

~~~mermaid
flowchart TD
    A["Accepted Grok development line"] --> P["PRE-CARRY: extract development contract"]
    S["Exact target stock"] --> B["Stock baseline proof"]
    P --> C["Classify semantics + process capabilities"]
    B --> C
    C --> D["Carry bootstrap: green on exact stock"]
    D --> E1["Closed development increment 1"]
    E1 --> G1["Accumulated proof GREEN"]
    G1 --> E2["Closed development increment 2"]
    E2 --> G2["Accumulated proof GREEN"]
    G2 --> EN["Closed development increment N"]
    EN --> GC["All planned proof + composition GREEN"]
    GC --> R["RECONSTRUCTED product + development process"]
    R --> Q["Final proof equivalence + capability continuity review"]
    Q --> V["Version-line PR"]
    V --> A1["Native protected-branch admission"]
    A1 --> N["Accepted Grok development line"]
    N --> L["Parallel Facts / Live evidence"]
    N --> D2["Optional distribution"]
~~~

## PRE-CARRY

PRE-CARRY defines what the next development line must preserve, change, or drop
before implementation begins.

### Resolve the continuation baseline

Start from the newest accepted version-line source. Use it directly when it is a
clean representation of accepted semantics.

A semantics-preserving baseline rewrite may be used when it removes accidental
history or representation noise. It must:

1. name the accepted source SHA;
2. classify every difference;
3. preserve accepted product behavior;
4. close any owner whose checked-in/runtime representation changes;
5. keep intentional product changes out of the rewrite;
6. remain disposable continuation history, not a second product trunk.

Distribution of the rewrite is irrelevant.

### Extract the development contract

Inspect the latest accepted line and record only capabilities relevant to
continuation. For each item capture:

- product behavior or development capability;
- owning seam;
- dependencies;
- current deterministic proof;
- optional Facts/Live evidence;
- admission/release role;
- `KEEP`, `UPDATE`, or `DROP` decision with rationale.

Do not use file identity as the contract. A workflow, harness, or implementation
may be reconstructed differently when stock architecture changes.

### Characterize the previous line

For every `KEEP` behavior that matters to the next line, identify the smallest
existing characterization/native proof that demonstrates the accepted behavior.

PRE-CARRY does not require inventing tests for already well-proven behavior, but
it must make the intended contract observable enough to reconstruct safely.

### Prove the target stock baseline

Before Grok reconstruction, exact target stock must pass the stock/native
baseline required for the affected development surface.

Then ask for every candidate behavior:

~~~text
Does target stock now own it?
    yes -> use stock; DROP the downstream mechanism

Does the product still require behavior stock does not provide?
    yes -> KEEP and reconstruct at the current seam

Did product intent change?
    yes -> UPDATE the contract intentionally

Was it only an experiment, workaround, or obsolete mechanism?
    yes -> DROP unless explicitly accepted
~~~

### Plan the dependency graph

Order development increments by semantic dependency, not historical commit
order.

A downstream increment must not appear before the owner it semantically depends
on. Independent owners may be ordered for review convenience.

## TDD and canonical history

TDD is encouraged during reconstruction:

~~~text
characterization/test
    -> RED locally
implementation
    -> GREEN
refactor
    -> GREEN
~~~

The canonical carry stack records the resulting green development milestone,
not every temporary RED step.

A test-first development sequence may therefore be folded into one closed
semantic commit containing the test, implementation, owned outputs, and proof.

Do not confuse useful local RED feedback with a requirement to publish broken
carry prefixes.

## Carry bootstrap

A carry branch starts from the exact target stock tag/SHA.

The first carry/process increment should establish the minimum progressive proof
machinery needed to validate the stack while remaining green on pure target
stock. It must not smuggle product semantics into a CI-only bootstrap.

Prefer one proof implementation reused by:

- progressive carry validation; and
- final version-line PR validation.

Avoid maintaining unrelated command lists whose proof surfaces can drift.

The bootstrap should make incomplete carry states explicit: only owners already
introduced by the current prefix are required, while baseline proof is always
required.

## Progressive reconstruction

For each planned development increment:

1. Read the current target-stock owner seam and native tests/generators.
2. Reconstruct the smallest meaningful product behavior.
3. Include owned generated/runtime-consumed outputs in the same increment.
4. Include native tests and deterministic harness pieces that prove the
   increment.
5. Activate that proof without disabling previous proof.
6. Run the accumulated proof for the current prefix.
7. Do not advance the canonical stack while that prefix is red.

No-path-conflict and clean patch application are not semantic proof.

### Owner closure

For each changed owner:

~~~text
changed behavior
    -> source-of-truth seam
    -> owned checked-in/runtime derived outputs
    -> native consistency/regression proof
~~~

Use stock generators when they exist. Do not maintain a repository-wide
generated-file checklist; closure follows ownership.

### Runtime evidence harnesses

Carry deterministic harness source with the development increment that owns the
behavior when practical.

For example, a hosted-tool increment may carry its deterministic Live-harness
scenario definitions even though real-provider Live execution remains a later,
environment-dependent evidence step.

This prevents the product from reaching the end of reconstruction with source
semantics present but its normal validation harness forgotten.

## Composition and RECONSTRUCTED

The final carry prefix must prove both product closure and development-process
closure.

### Product closure

- every selected semantic is `KEEP`, `UPDATE`, or `DROP`;
- every retained/updated owner is closed;
- all owned derived outputs are current;
- all owner-native proofs pass;
- cross-owner composition passes.

### Development-process closure

- progressive deterministic proof is complete;
- the full deterministic proof required for version-line PR admission is
  available;
- inherited admission, Facts, Live, and release/distribution capabilities have
  been preserved, reconstructed, or explicitly dropped with rationale;
- the line has enough process machinery to continue normal development after
  admission.

`RECONSTRUCTED` means both closures are complete on exact target stock.

It does **not** mean that real-provider Live has run, that binaries have been
built, or that distribution has succeeded.

Do not add product states merely to mirror CI phases.

## Final proof equivalence

Before version-line admission, run the completed carry candidate through the
full deterministic proof contract expected by a version-line PR.

If the version-line PR would require deterministic proof that the final carry
does not run, the carry process is incomplete. Fix the shared proof model rather
than accepting two divergent validation authorities.

A useful implementation shape is:

~~~text
shared native tests / proof commands
        |                      |
        v                      v
progressive carry wrapper   version-line PR wrapper
        |                      |
        +------ same full deterministic proof at completion ------+
~~~

Do not require the wrappers themselves to be identical.

## Capability continuity review

After process closure, perform a short guardrail review against the latest
accepted line.

For every relevant capability, classify `KEEP`, `UPDATE`, or `DROP`:

- required PR proof and public aggregate checks;
- branch/ruleset admission controls;
- Facts/backend-observation capability;
- Live/runtime-validation harness and invocation capability;
- manual diagnostic entrypoints;
- artifact-to-Live relationships;
- release/distribution capability;
- external prerequisites such as secrets or artifact dependencies.

Also classify the role:

~~~text
admission-required
    = must be usable before source admission

parallel evidence
    = should remain available but execution does not create source authority

distribution-only
    = optional delivery capability
~~~

This review is a final anti-omission guardrail, not the primary carry mechanism.
Most continuity should already have emerged naturally from the progressive
development stack.

## Version-line admission

The version line `grok/rust-vX.Y.Z` records accepted source for that stock
generation.

Admission uses native repository review, the full deterministic PR proof, and
the repository's protected-branch/ruleset controls.

### First authoritative head

Creating a branch with a version-line name does not make its head authoritative.

The first authoritative head must have an explicit relationship to the required
version-line PR proof.

If platform constraints require a target ref to exist before the first PR, that
ref is provisional. It becomes authoritative only after a non-empty,
PR-proven transition enters through the normal protected-branch path.

Do not use direct branch creation, a release build, Live success, or an artifact
as a substitute for the first admission proof.

### Accepted source

After the PR-proven change is admitted through the native branch controls, the
resulting version-line head is accepted source authority.

That accepted source may immediately seed future PRE-CARRY/baseline resolution.
Artifact availability is irrelevant to that source continuity.

## Evidence classes

Keep these evidence classes separate.

### Deterministic implementation proof

Native tests, lint, generated consistency, harness unit tests, and composition
that can run without the real provider.

This is the proof that must compose progressively and be equivalent at final
carry/version-line PR boundaries.

### Facts

Facts are backend observations. They may change product decisions or evidence
classification, but do not create source authority.

### Real-provider Live

Live verifies runtime/backend behavior. Carry the capability when it is part of
the accepted development process, but do not require every semantic prefix to
run it.

A Live failure after source admission is actionable runtime evidence; classify
the failing owner instead of retroactively treating an artifact as source
authority.

### Distribution smoke

Distribution smoke verifies packaged artifact usability. It is derivative of a
selected source SHA and is governed by [distribution.md](./distribution.md).

## Distribution boundary

The ability to produce and validate human-usable packages may be part of the
development process that must be preserved or intentionally changed.

Actual binary distribution remains optional.

~~~text
accepted source SHA
        v
optional build/package
        v
downloadable artifacts
~~~

Never infer authority in the reverse direction.

## Mechanism discipline

Carry a development process, not a second workflow platform.

Prefer:

1. native owner tests;
2. shared deterministic proof commands;
3. thin progressive-carry and version-line wrappers;
4. native GitHub review/rulesets for admission;
5. separate Facts/Live evidence where environment-dependent;
6. simple distribution jobs when humans need artifacts.

Avoid proof ledgers, custom workflow-state machines, post-hoc provenance
reconstruction, and helpers whose only purpose is to mirror GitHub state.

## Historical diagnosis

The newest accepted line is the normal process and semantic reference.

Inspect older version lines only when the latest accepted line, current doctrine,
target stock, and directly relevant evidence cannot answer a concrete question.
Stop once the reason for a behavior or capability is understood.

Historical implementation sequences are evidence, not instructions to replay
them.

## grok/main

`grok/main` is the current process overlay plus optional short-lived
experiments on practical stock main.

Keep it cheap to rebase and easy to understand.

~~~text
current practical openai/codex main
+ current Grok process overlay
+ optional active experiments
~~~

Prefer rebase when updating stock. Prune finished experiments. Preserve the
current process overlay.

Changes to `grok/main` do not themselves promote product semantics into a
version line.

## Feedback

Carry work should improve the development process when it reveals a reusable
lesson.

Preserve the lesson, not the accidental patch:

~~~text
observed carry failure
    -> identify violated development invariant
    -> update current doctrine/proof architecture
    -> apply the improved process to future carries
~~~

Do not encode one-off historical accidents as permanent machinery.

## Backports

Historical accepted lines are normally frozen.

Backport only for a concrete support, correctness, security, or explicitly
requested distribution requirement. Use the historical line's own architecture
and proof surface rather than turning backport work into a new carry-forward.
