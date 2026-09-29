# Grok 0.157.1 semantic continuation baseline

This branch is the reconstructed semantic continuation baseline for carrying
Grok product behavior from stock Codex `rust-v0.157.1` onto later stock
versions.

It is not a binary release state. Artifact production does not establish or
gate its continuation role.

## Source identities

Accepted 0.157.1 source:

```text
grok/rust-v0.157.1
b187b716787cf078a4c0f51986eb3555e9f6c6de
tree 5cc687fa15f7d464a7951bc4e7c51fc4a3763ff7
```

Exact stock base:

```text
openai/codex rust-v0.157.1
36650394c5b38c2990ccf2a3457165ca3e9d9726
tree 1d4f30646b7a0ceb5df1c36a837de371ffa9acdc
```

## Reconstructed semantic stack

The baseline is rebuilt from stock by current semantic owner rather than by
preserving the previous implementation history:

```text
dbcadd3f  feat(grok): establish provider and wire compatibility layer
9ec3a0a5  feat(core): add structured edit with conditional writes
68794e5f  fix(core): accept exact whole-number JSON tool arguments
1c3cb4c3  feat(grok): adapt tool projection and hosted-call semantics
58568453  fix(core): close turn input queues at the snapshot boundary
e7cfc78f  feat(grok): define the product profile and bundled catalog
bc5ac54b  test(grok): capture backend facts and semantic live contracts
```

The final baseline metadata commit adds only this document and the Grok index.

## Ownership corrections made by the rewrite

The reconstruction intentionally changes commit ownership without changing
product behavior:

- structured-edit registration and its tool-plan tests move into the
  structured-edit semantic owner;
- exact whole-number JSON handling becomes its own provider-neutral core fix;
- Grok flat-tool/hosted semantics no longer claim ownership of the whole-number
  behavior;
- bundled `config.toml.example` and `models.json` are treated as runtime
  product profile/catalog semantics, not as binary packaging;
- Facts, Live harness code, Stories, architecture, and whitelist remain
  semantic evidence while workflow scheduling is excluded.

## Cargo.lock normalization

The accepted source contains 155 workspace/path package entries rewritten from
the upstream checked-in `0.0.0` representation to `0.157.1`.

This baseline keeps the upstream `0.0.0` representation.

The legitimate dependency change remains:

```text
codex-tools -> "sha1 0.10.6"
```

No dependency edge or checksum is removed by that normalization.

## Intentionally excluded process and distribution machinery

The semantic baseline does not carry machinery whose purpose is orchestration
or human-download packaging, including the 0.157.1 downstream additions for:

- Grok distribution build/upload workflows and composite build action;
- Facts workflow scheduling;
- full-CI workflow scheduling changes;
- INSTALL and launcher wrappers;
- branch-local release-state documentation;
- the distribution-era `grok-rust-chore` justfile helper.

Those remain historical evidence on the accepted 0.157.1 line when needed.
They are not inputs to semantic continuation.

## Verified accepted-source delta

A recursive blob comparison against accepted
`grok/rust-v0.157.1@b187b716787cf078a4c0f51986eb3555e9f6c6de`
found 25 changed paths in this semantic baseline.

They classify completely as:

- one semantics-preserving representation correction:
  `codex-rs/Cargo.lock`;
- process/distribution removal or stock restoration:
  Grok build/upload workflows, Facts scheduling, downstream
  `rust-ci-full.yml` scheduling changes, launch/install assets, the legacy
  branch-local release document, and the justfile formatting helper;
- continuation-oriented product/evidence documentation, including this baseline
  record.

`.github/workflows/rust-ci-full.yml` and `justfile` exactly match the stock
`rust-v0.157.1` blobs.

No Rust/runtime implementation path differs from the accepted 0.157.1 source
other than the Cargo.lock representation described above.

## Documentation alignment

The product docs in this baseline are continuation-oriented:

- architecture and whitelist do not treat packaged artifacts as semantic
  authority;
- the whitelist stock anchor is `rust-v0.157.1`;
- Stories name semantic tests without making workflow placement authoritative;
- there is no branch-local `release.md` authority.

Current product-evolution and binary-distribution policy is maintained on
`grok/main`.

## Use for the next carry

Use this baseline as semantic input, not as an implementation patch series:

```text
this 0.157.1 semantic baseline
        +
explicit product decisions after 0.157.1
        +
selected accepted experiments, if any
        +
exact target stock
        v
semantic reconstruction at target-stock owners
```

For each semantic, first ask whether target stock now owns it. Retain only the
smallest downstream behavior still required at the current stock seam.

Do not mechanically rebase this tree onto a later tag and do not replay old
probe/fix/distribution commits merely because they existed.

If a change alters accepted product behavior, record it as an explicit product
decision. Do not hide it inside baseline cleanup.
