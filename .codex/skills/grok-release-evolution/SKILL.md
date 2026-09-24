---
name: grok-release-evolution
description: Evolve the downstream Grok product across stock Codex main and exact rust-v* tags. Use for Grok experiments on grok/main, semantic carry-forward to a version line, diagnosing carry resistance with historical successful releases, release proof, or feeding release lessons back to grok/main. Do not use for ordinary stock Codex work.
---

# Grok Release Evolution

Use this skill for the Grok product lifecycle. The goal is semantic continuity
across a fast-moving stock Codex without turning Git history into product
architecture.

## Read the Authorities

Before changing code, read only the authorities needed for the current mode:

- `grok/docs/architecture.md` for runtime and product invariants.
- `grok/docs/carry-forward.md` for branch roles, semantic selection, history
  use, and feedback rules.
- `grok/docs/release.md` for proof and delivery.
- `grok/docs/request-whitelist.md` when Responses, tools, history, search,
  SSE, Images, or Provider API projection is involved.
- The actual current tree and native tests at every seam you touch.

Treat those documents and the current repository state as authorities. This
skill is the execution protocol, not a second copy of product truth.

## Branch Model

Keep the roles distinct:

```text
grok/main
    = integration / experimentation head
    = stock Codex main + current Grok work
    = primary source of candidate downstream semantics
    = not release authority

grok/rust-vX.Y.Z
    = Grok release line for exact stock rust-vX.Y.Z
    = release authority for that exact version after its proof succeeds
    = historical successful composition after release
    = normally frozen

carry/grok-rust-vX.Y.Z
    = temporary semantic translation and review workspace
    = targets grok/rust-vX.Y.Z
    = has no product authority of its own
```

Do not model these branches as a promotion chain. A proven version line is not
promoted into `grok/main`, and `grok/main` is not copied wholesale into a
version line.

## Choose the Mode

### 1. Integration / Experimentation

Use this when changing `grok/main`.

- Work against the current stock-main seams.
- Experiments may lead release lines; being on `grok/main` does not make a
  behavior release-required.
- Keep Grok-specific behavior at the narrowest verified Provider boundary.
- Keep independently correct fixes provider-neutral.
- Record durable product or process lessons in the owning Grok docs.
- Do not backport the change to historical release lines unless the user
  explicitly requests a backport for a concrete support reason.

### 2. Carry Forward to an Exact Tag

Use this when preparing `grok/rust-vX.Y.Z`.

Start from the exact upstream tag/SHA and a matching `carry/*` branch. Read
`grok/main` as the primary source of candidate downstream semantics, not as
a tree to replay.

For each candidate behavior, classify it:

```text
stock-owned now
    -> use stock; drop the downstream mechanism

release-required downstream semantic
    -> carry the smallest semantic at the target stock seam

main-only experiment
    -> do not carry

obsolete workaround / historical intermediate state
    -> drop

provider-neutral correctness fix
    -> check whether target stock owns it;
       if not and the release still depends on it, retain it provider-neutrally

unclear
    -> enter resistance diagnosis; do not invent compatibility glue
```

Preserve the semantic, not the old implementation shape. No-path-conflict is
not proof of semantic compatibility, and nearby stock churn is not a reason to
patch Grok.

Construct reviewable commits by current semantic owner. Fold historical
probe/fix/cleanup sequences into the settled owner instead of replaying them.

### 3. Resistance Diagnosis

Use historical release lines only when current `grok/main` intent plus the
target stock architecture do not explain how to carry a semantic cleanly.

Inspect the smallest useful history:

1. The nearest successful `grok/rust-v*` release that implemented the
   relevant behavior.
2. An earlier successful release only when needed to locate a transition.
3. Stock changes between the relevant fixed points.
4. Grok semantic changes between those releases.

Ask:

- Why did the downstream mechanism exist?
- Was it a durable Grok contract or a workaround for an old stock seam?
- When did the stock responsibility move?
- What user-visible semantic survived the transition?

Historical releases are diagnostic evidence, not carry-forward starting
points. Do not mechanically cherry-pick or reconstruct an old implementation
because it once shipped successfully.

If a clean carry would require changing a current release contract, stop and
report the concrete conflict instead of inventing a compatibility layer.

### 4. Release Proof

Keep proof authorities separate:

```text
Facts
    = backend observations

PR deterministic tests
    = code proof at native owning seams

version-line push
    = exact target distributions

Live
    = real-provider composition proof on the exact Linux artifact
```

A green PR is not a proven release. After an explicitly authorized merge to
`grok/rust-vX.Y.Z`, the version-line push must build the shipped artifacts
and Live must consume the exact Linux artifact.

Use native Rust tests, native Go tests, and GitHub Actions outcomes. Do not add
proof ledgers, test-output parsers, publisher state machines, manual merge-SHA
checkout logic, or custom reimplementations of Actions filtering.

Do not merge, publish, mutate another branch, or repeatedly monitor CI unless
the user has authorized that action.

### 5. Feed Release Lessons Back to grok/main

A release carry can discover improvements that belong in the active
integration head. Treat these as semantic feedback, not branch promotion.

For each feedback candidate:

1. State the lesson independently of the release patch.
2. Inspect current stock main.
3. Ask whether stock main already owns the behavior.
4. Confirm the problem or opportunity still exists.
5. Find the current stock-main seam.
6. Re-express the semantic there with its owning tests.

Preserve the lesson, not the patch. Do not merge a release branch into
`grok/main` or cherry-pick release commits by default.

A completed release report should include:

```text
feedback candidates for grok/main:
```

Use `none` when there are no such lessons.

## Stable Grok Boundaries

Unless the current architecture documents explicitly change them, preserve:

```text
WHO   = model_provider / Provider profile
HOW   = WireApi::GrokResponses -> ApiDialect::Grok
WHERE = resolved base_url / routing
```

Do not select Grok behavior by display name, hostname, TrustedTunnel hostname,
or model name.

Keep the shipped catalog through stock `model_catalog_json`. Do not add Grok
models to the stock OpenAI models-manager catalog or fork the stock TUI picker
unless the current stock seam can no longer satisfy a concrete product
requirement.

Keep delivery Actions-native and installation document-driven. Do not
resurrect removed release scripts, installers, publishers, ledgers, or summary
parsers merely because they exist in history.

## Completion Reporting

For a carry-forward, report at least:

```text
upstream fixed point:
working branch:
PR:
semantic commits:
stock-owned mechanisms dropped:
Grok-specific semantics retained:
provider-neutral fixes retained:
provider-neutral fixes dropped:
model catalog result:
workflow/delivery result:
documentation updated:
feedback candidates for grok/main:
known unresolved issues:
```

Also state explicitly which authoritative branches were not modified.
