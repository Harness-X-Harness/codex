# Grok image generation and same-Thread edit

## User story and boundary

As a Grok user, I can request a recognizable picture and later edit that picture
in the same conversation. The exact selected Codex binary and bound Grok model
own generation, history and image handling. Acceptance observes the public
result and its visible content, not a scripted sequence of internal tool calls.

The current `image-primary` and `image-pinned` runners use an isolated
image-capable profile and generated catalog. They do not consume unchanged
shipped Grok profile/catalog assets. A pass is scoped to that isolated product
path; unchanged shipped-profile composition remains a separate, unproven goal.
This Story does not prove Mini routing/accounting or Grok Build.

## Acceptance

Given a fresh isolated environment and the identified binary/model:

1. A natural request asks for a dog on a plain background. Its Turn completes
   and supplies a completed public image with a user-accessible, decodable PNG.
2. A fresh judge Thread receives those actual image pixels and the question
   "Is there a dog in the photo?" It receives no producer conversation/history.
   The turn uses the SDK's formal output schema `{"type":"boolean"}`; its
   completed final response must be the JSON boolean `true`.
3. The original generation Thread receives a natural request to add a red hat
   while keeping the dog and background. Its Turn completes with a public image
   and matching saved PNG.
4. Another fresh judge Thread receives the edited pixels and the question
   "Is there a dog wearing a red hat in the photo?" The same boolean output
   schema applies, and its completed final response must be `true`.

The LLM judges the visual proposition from actual pixels; the harness only
validates the boolean result. The criterion is intentionally known to the judge,
not the producer's claimed answer or history. `false`, missing/malformed output,
null, strings, numbers or objects do not establish the proposition. There is no
breed dictionary, lexical description matcher, repair call or fallback judge.
Schema conformance is not itself visual correctness: judge mistakes remain
possible, and retained PNGs and public replies permit inspection. A pass is not
a pixel-perfect measure of every aspect of an edit.

The saved file must match the public image payload and remain safely readable
inside the owned image directory. Decoding and byte/pixel budgets are resource
bounds, not visual correctness. Generation and edited digests identify evidence;
unequal hashes, a particular history rendition, an exact selector argument,
raw extension events and an inventory of pre-existing images are not extra
acceptance conditions. Intermediate errors and optional tool activity do not
invalidate a subsequent complete, correct result within the same invocation.

## Partial success and failure boundaries

An image endpoint response without a completed public image, an unreadable or
invalid PNG, an unsuccessful Turn, missing or incorrect independent recognition,
or generation without the requested edited result does not complete the Story.
A generation prompt echoed as an answer cannot replace recognition in a fresh
conversation. Wrong-thread evidence cannot complete the submitted operation.

Each semantic input is submitted once. The runner does not retry or replace an
incomplete generation, edit or recognition Turn. Product-internal attempts and
optional viewing remain product behavior, not a runner script. No fallback to
another Provider or unrelated image may stand in for the requested result.

## Proof plan and evidence

The proof has four semantic Turn submissions across three fresh Threads, in one
owned process and isolated environment. The generation and edit share the first
Thread; each recognition uses its own new Thread. The scenario catalog owns the
executable budget. A separately authorized real-backend run is required; these
instructions do not authorize one.

- [`ImageGenerationEdit`](../../live/image.go), the
  [SDK runner](../../live/sdk_image.go) and
  [artifact reader](../../live/image_artifact.go) own the result oracle.
- [`TestGrokImageGenerationEdit` and negative controls](../../live/image_test.go)
  use a fake public App Server and validate submission, independent recognition,
  completion and rejection of wrong/incomplete results.
- [`TestImagePinnedFixture`](../../tests/integration/image_test.go) executes the
  selected real Codex against controlled Responses/Images HTTP. Its synthetic
  PNGs and scripted visual answers validate transport, actual image input,
  same-thread edit and independent recognition orchestration, not real vision.
- [Rust image preparation](../../../codex-rs/core/src/image_preparation.rs),
  [public image integration](../../../codex-rs/app-server/tests/suite/v2/imagegen_extension.rs)
  and stock `ImagesClient` regressions separately own exact tool/ABI, codec,
  preparation, lifecycle and request/response compatibility. Their internal
  assertions do not dictate the natural Live execution path.
- [`TestLive/image-primary` and `image-pinned`](../../tests/live/live_test.go)
  exercise the isolated real-backend path only when authorized.

Evidence contains safe subject metadata, completion/recognition booleans,
counts, artifact digests and bounded failure classification under the
[failure-evidence contract](../../TESTING.md#live-failure-evidence). It does not
put prompts/replies/images in the verdict summary. The separate bounded diagnostic
bundle retains this synthetic task's actual prompts, schema, public judge reply,
parsed decision and PNGs before cleanup, including on rejection. Credentials,
private reasoning and full raw transport remain excluded.
Offline/control passes are not executed real-backend evidence and do not
certify another source, target, model, arbitrary images, multi-image editing,
transparent output, performance or absence of internal attempts.

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
