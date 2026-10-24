# Grok image generation and same-Thread history edit

The implemented image tool supports generation and same-thread history edit.
Offline Go tests validate its observer, Rust tests own image preparation and
product behavior, and real Codex with controlled HTTP tests the pinned-model
composition. A packaged real-xAI result requires a separately identified Live
run; none of those controlled tests establishes it.

The current `image-primary` and `image-pinned` Live runners use the selected
binary with an isolated image-capable profile and generated model catalog, not
the unchanged shipped Grok profile. Their result can establish generation and
history edit in that isolated context. The shipped-profile precondition below
is a retained product acceptance goal and is not proved by these two routes,
even when their isolated observations pass.

## User story

As a Grok user, I can generate an image and later edit that generated image in
the same Thread through the stock Codex image tool.

## Real path

```text
exact packaged Grok artifact
  -> Thread bound to the supported Grok Provider profile
  -> stock Codex image generation
  -> completed, user-accessible, decodable image result
  -> later Turn in the same Thread
  -> stock history supplies the generated image to the stock Codex image edit
  -> completed, user-accessible, decodable edited image result and Turn
```

This Story covers Grok Provider image generation and history edit. It does not
prove Mini authorization, routing, grants, transport, or accounting. It does
not cover Grok Build.

## Acceptance

**Given** a packaged Grok artifact with the shipped Grok Provider profile,

**when** the acceptance runner submits one natural image-generation Turn and,
after it completes, one natural edit Turn in the same Thread,

**then** the stock Codex image-tool path completes both operations, the later
edit uses the prior generated image from canonical Thread history, each result
has a verified image MIME and matching codec, each result is available to the
user, and both Turns reach a completed terminal state.

## Partial success is not completion

- The Provider emits a function request but no canonical image result
  completes.
- An image endpoint returns data but no matching image result is available to
  the user.
- Generation completes but the later edit does not use the same Thread's
  canonical image history.
- An image result exists but its Turn does not complete.
- Deterministic contracts pass without the packaged external result.

## Material failure boundaries

- The runner does not resubmit a failed or incomplete generation or edit Turn.
- No fallback to another Provider, image tool, image lifecycle, history store,
  or media store can complete this Story.
- Provider response count, function-call count, image-item multiplicity,
  ordering, timing, and retry shape are diagnostics unless an owning interface
  makes them user-visible.

## What this does not prove

This Story does not prove another Grok source or target, future xAI behavior,
multi-image edit cardinalities, transparent output, a latency objective,
Grok Build, Mini accounting, or the absence of valid Provider-internal
attempts.

## Proof plan

### Preconditions

- Native Grok image, catalog, projection, codec, MIME, stock history-edit, App
  Server lifecycle, and stock Provider-control tests passed for that source.
- The release-bundled image-capable model and the supported Grok Provider
  profile.
- The selected invocation fits the runner's [observation budgets](../../live/README.md).
  Exceeding a budget leaves the Story unproven; it does not establish a product
  defect or narrow supported product output sizes.

### Proof-run invocation budget

One generation Turn and one later edit Turn in the same Thread. The runner
does not retry or replace either semantic Turn.

### Secret-safe evidence

The [current failure-evidence contract](../../TESTING.md#live-failure-evidence)
owns the bounded, secret-safe observations returned by this runner. Partial
observations do not claim Story completion or assign defect ownership.

## Stock compatibility control

The exact deterministic gate runs the pre-existing stock `ImagesClient`
generation and edit request/response regressions at the changed shared client
seam. Provider catalog, capability, flat projection, image dialect, and
effective edit budget checks remain Grok-specific. The `image_gen.imagegen`
tool identity and App Server item-shape compatibility remain a separate
deterministic child proof; they are not the unique Live image-result oracle.

## Executable contract

[`ImageGenerationEdit(ctx, Options)`](../../live/image.go) and
[`TestGrokImageGenerationEdit`](../../live/image_test.go) own the offline observer.
[`TestImagePinnedFixture`](../../tests/integration/image_test.go) executes real
Codex with a controlled Responses/Images backend: generation, edit, exact image
and history hashes, and same-thread lineage. [Rust image preparation](../../../codex-rs/core/src/image_preparation.rs)
and [public image integration](../../../codex-rs/app-server/tests/suite/v2/imagegen_extension.rs)
own the product transformation/lifecycle. [`TestLive/image-primary` and
`image-pinned`](../../tests/live/live_test.go) own real-backend execution under
the isolated image profile created by [`writeFixture`](../../live/fixture.go).
They do not establish unchanged shipped-profile/catalog composition. A result
must keep that distinction explicit rather than credit the full shipped-profile
acceptance goal from the isolated gate.
The deterministic public-stdio scripts exercise the runner with an independent
image-capable model fixture. They cannot establish the shipped model catalog,
actual Grok HTTP egress, backend visual correctness, or packaged Story success.

The observer requires decodable image results, exact public payload/saved-byte
agreement, fresh user-accessible artifacts, a distinct edited result, and
correlated canonical same-Thread history selection. Stock media preparation
may resize or re-encode history images; it does not invalidate those artifact
requirements. The [image observer](../../live/image.go) and [artifact checks](../../live/image_artifact.go)
own correlation, codec, preparation, recovery and resource checks.

The consumed fixture enables image generation and the existing view-image tool,
with other tools disabled. An optional view must identify an already observed
artifact and return its known image bytes or prepared rendition. Viewing it is
not generation credit and does not itself prove an edit. Offline controls cover
view-before-edit and view-after-edit without requiring that auxiliary route.
The controlled binary fixture still exercises direct generation/edit; it is not
evidence that the general packaged profile executed every possible view route.
Target Turns are found by unique identity, not by history length or position.
Equivalent notifications and valid internal recovery do not consume another
proof-run invocation.

## Product image boundary

The explicit Grok dialect selects image HTTP semantics, independently of Provider
names or URLs. Stock image lifecycle and history remain authoritative. Grok
supports up to three inline edit references; selected file references fail
explicitly rather than exposing an older image. Stock file-backed editing,
five-reference support and transparency behavior remain unchanged. Grok rejects
any explicit `transparent_background` key, including false/null, before dispatch;
arguments must be JSON objects.

The [Grok Images adapter](../../../codex-rs/codex-api/src/grok_images.rs) owns request and
normalization policy: `grok-imagine-image-2.0`, `b64_json`, retained single-image
defaults and fail-closed unsupported options. Completed artifacts use validated
PNG; supported still JPEG/WebP are normalized to PNG, while animated WebP and
malformed/unsupported codecs fail without a successful artifact. PNG alpha and
the existing orientation/color handling remain intact. The local 32 MiB byte
and decoded-pixel budget is a resource policy, not an xAI service limit.

The shared [schema owner](../../../codex-rs/tools/src/json_schema/types.rs)
and [bounds tests](../../../codex-rs/tools/src/json_schema_bounds_tests.rs) own
exact numeric declarations and `maxItems`, including their non-image consumers.
These declarations do not replace runtime argument validation; malformed bounds
must follow the owning registration/catalog rejection or fallback contract.
