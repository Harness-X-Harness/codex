# Grok distribution

This document is the canonical current policy for Grok binary distribution.

Distribution is part of the **software-development capability** that may need to
survive carry-forward, but actual artifact production remains optional.

That distinction is fundamental:

~~~text
carry the ability to distribute when it is an accepted development capability
!=
require every accepted source to be distributed
~~~

Product-development evolution is governed by
[carry-forward.md](./carry-forward.md).

## Authority direction

Source authority flows one way:

~~~text
accepted source SHA
        v
optional build/package
        v
artifact
        v
human use
~~~

Never infer authority in reverse.

- artifact success does not make source canonical;
- artifact failure does not revoke accepted source;
- missing artifacts do not block future carry-forward;
- Live or smoke success on an artifact does not replace source admission.

## Distribution capability in carry-forward

When the previous accepted development line can produce supported human-use
packages, PRE-CARRY must classify that capability as `KEEP`, `UPDATE`, or
`DROP`.

The carry may therefore need to reconstruct:

- target build action(s);
- package staging rules;
- launch/install assets;
- artifact naming;
- artifact-level smoke;
- artifact-backed Live wiring where it serves runtime/release evidence.

This is process continuity.

It does not require running those builds before `RECONSTRUCTED` unless they are
the only available deterministic proof for a product owner, which should be
avoided where practical.

## Inputs

A distribution operation starts from an explicitly selected exact source SHA.

For normal human delivery, that SHA should be an accepted version-line head.

Diagnostic packaging of another SHA is allowed when the purpose is explicit,
but the artifact must clearly identify its source.

Distribution never decides which source is accepted.

## Outputs

An artifact should contain exactly what a human needs for the supported target.

Target/platform coverage follows actual users, not symmetry.

Every artifact should be traceable to:

- repository;
- exact source SHA;
- target platform;
- run identity when useful.

Package metadata and checksums are identification/integrity aids, not semantic
authority.

## Build and staging design

Prefer a small, explicit flow:

~~~text
selected source SHA
    -> build requested target once
    -> stage complete human-use package
    -> optional package smoke
    -> upload/publish
~~~

Avoid duplicated builds of the same target merely to satisfy different workflow
phases.

When runtime Live must prove the packaged binary, prefer same-run artifact flow:

~~~text
Linux build
    -> Linux package artifact
    -> Live consumes that exact artifact
~~~

The workflow DAG and run-local artifact namespace are sufficient binding when
GitHub Actions is the trusted execution environment. Do not rebuild historical
provenance with custom ledgers or PR/run selectors.

## Live roles

A Live check must declare its role.

### Product/runtime Live evidence

Verifies a user-visible product contract against the real backend.

Examples include provider routing, hosted search, reasoning continuation, image
behavior, or another runtime semantic.

Its harness capability may be carried with the owning semantic increment.
Execution may happen later because it depends on backend availability/secrets.

### Artifact/release Live evidence

Consumes the packaged artifact and proves that the delivered composition works.

This may be the same scenario code as product Live, but its evidence role is
different because the subject under test is the package produced by the release
path.

Neither role creates source authority.

## Facts

Facts are real-backend observations independent of package availability.

They may inform product decisions and evidence classification. They are not
distribution state.

A Facts workflow can be a carried development capability without being part of
artifact production.

## Failures

Classify failures by owner.

~~~text
source/native test failure
    -> product/development proof failure

build/staging failure
    -> distribution capability or artifact failure

artifact smoke failure
    -> package usability failure

real-provider Live failure
    -> runtime/backend evidence failure
~~~

Do not translate a packaging failure into semantic rejection without direct
evidence of a product defect.

## Human installation

Human-use packages should document:

- which artifact/target to download;
- how to restore executable bits when archive transport drops them;
- expected launcher/binary layout;
- required config/catalog/profile assets;
- product Home requirements;
- how to start the product.

Installation must not silently migrate or overwrite unrelated user state unless
that is an explicit product decision.

## Retention

Artifacts are derivatives and may be cleaned according to practical retention
needs.

Deleting an artifact does not delete or invalidate the accepted source from
which it was built.

## Mechanism discipline

Do not create a second release-control plane.

Avoid:

- release-proof ledgers;
- publisher state machines;
- custom source-promotion logic based on artifact status;
- historical PR/run reconstruction used as source admission;
- workflow-state models duplicating GitHub's native state.

Prefer native branch admission, shared deterministic proof, a simple build DAG,
and explicit artifact identity.
