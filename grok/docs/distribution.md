# Grok binary distribution

This document is the canonical current policy for Grok binary distribution.

Distribution has one purpose: make binaries convenient for humans to download
and use.

It does not establish product semantics, source authority, semantic acceptance,
or eligibility for the next carry-forward.

Product evolution policy lives in [carry-forward.md](./carry-forward.md).

## Authority direction

The authority direction is strictly one-way:

~~~text
accepted source SHA
        v
build and package
        v
downloadable artifact
        v
human use
~~~

Never infer the reverse.

- A successful artifact does not make source canonical.
- A failed or missing artifact does not make accepted source non-canonical.
- Distribution does not gate the next carry-forward.
- Rebuilding the same semantics under a different history representation is not
  required merely because the original representation produced an artifact.

## Inputs

A distribution job starts from an explicitly selected source SHA.

That SHA should normally be an accepted version-line source. A diagnostic or
temporary build may use another SHA when the purpose is clear, but the artifact
must identify exactly what source it came from.

Distribution policy does not decide which source is semantically accepted.
That decision belongs to product evolution, review, and version-line admission.

## Outputs

A complete distribution artifact should contain the binaries and product assets
needed by the intended human user.

The concrete target set and package contents may evolve with actual users.
Do not expand target coverage merely to create symmetry.

Each artifact should be traceable to:

- repository;
- exact source SHA;
- target platform;
- workflow/run identity when useful for retrieval.

Checksums and metadata are useful when they help humans verify or identify what
they downloaded. Do not turn them into a parallel semantic authority.

## Build failures

A build failure means:

~~~text
artifact unavailable for that source/target
~~~

It does not mean:

~~~text
source semantics rejected
product state revoked
next carry blocked
~~~

Fix a distribution failure when humans need that artifact. Do not force product
evolution to wait on an unrelated packaging problem.

## Live checks

Live checks must declare which role they serve.

### Semantic Live evidence

A Live check is semantic evidence when it verifies a product behavior against
the real backend, such as Provider binding, hosted search, reasoning/history,
images, or another user-visible contract.

That evidence belongs to product validation. It should be associated with the
semantic owner and may run before or independently of binary distribution.

### Distribution smoke evidence

A Live or smoke check is distribution evidence when it verifies the packaged
artifact itself:

- the expected executable is present;
- the launcher works;
- required bundled assets are found;
- the packaged binary can start and perform the intended smoke scenario.

It is often efficient to run this against the same-run artifact. That is an
implementation convenience, not a product-authority boundary.

Do not use artifact-level smoke success to select the next semantic baseline.

## Facts

Facts are backend observations.

They may inform product decisions and semantic validation. They are not
distribution state and do not become more authoritative because an artifact was
built or downloaded.

## Workflow design

Keep distribution workflows small.

Prefer:

1. build the requested target exactly once;
2. stage the complete artifact;
3. run only the artifact-level checks that protect download usability;
4. upload or publish the artifact where humans can retrieve it.

Do not add:

- release-proof state machines;
- proof ledgers;
- provenance reconstruction whose only purpose is to promote source authority;
- historical PR/run selectors used to decide whether source is semantically
  valid;
- custom workflow-state models that duplicate GitHub state.

Repository review and semantic tests may run in the same workflow for
convenience, but their authority remains separate from distribution.

## Version lines and historical release documents

`grok/rust-vX.Y.Z` is a source version line, not a distribution state.

Historical version lines may contain `grok/docs/release.md` describing the
delivery process used at that time. Those files are historical branch-local
operation evidence. They do not override this current distribution doctrine or
the current product-evolution policy on `grok/main`.

Where a current version-line workflow still uses older names such as
`release`, `SUCCESSFUL`, or `release proof`, interpret those names as legacy
workflow vocabulary until the implementation is simplified. They must not be
used to infer semantic authority.

## Human installation

Installation documentation should answer the practical questions a human needs:

- which artifact to download;
- how to make launchers executable when archive transport drops mode bits;
- which product Home/config/catalog files to use;
- how to start the binary.

Installation must not silently migrate or overwrite existing user state unless
that is an explicit product requirement.

## Retention and cleanup

Distribution artifacts and temporary distribution branches may be cleaned
according to practical retention needs.

Deleting an artifact does not delete or invalidate the accepted source state
from which it was built.
