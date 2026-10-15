# Grok image generation and same-Thread history edit

Retained contract from accepted 0.157.1 (`b187b716787cf078a4c0f51986eb3555e9f6c6de`).
C5 restores deterministic scenario support; this Story is **not proven** for the
current source. C7/#322 owns Grok Responses tool/media/history composition,
C8 owns packaging, #339 owns packaged invocation, and #331 owns actual backend execution.

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

The retained packaged/backend acceptance requirement is unchanged: GREEN records
nothing. A RED run preserves for 7 days the redacted session JSONL and rejected
request key-path shapes with backend status and redacted error text; no prompt,
model output, credential or Thread ID is recorded. That artifact-retention path
is not implemented or proven by C5's deterministic scripts.

The current deterministic runner returns only bounded, secret-safe partial
observations described in its [evidence contract](../../live/README.md).
Those observations do not claim Story completion or assign defect ownership.


## Stock compatibility control

The exact deterministic gate runs the pre-existing stock `ImagesClient`
generation and edit request/response regressions at the changed shared client
seam. Provider catalog, capability, flat projection, image dialect, and
effective edit budget checks remain Grok-specific. The `image_gen.imagegen`
tool identity and App Server item-shape compatibility remain a separate
deterministic child proof; they are not the unique Live image-result oracle.

## Executable contract

`ImageGenerationEdit(ctx, Options)` and the consumed
`TestGrokImageGenerationEdit` in [`grok/live`](../../live/README.md).
The deterministic public-stdio scripts exercise the runner with an independent
image-capable model fixture. They cannot establish the shipped model catalog,
actual Grok HTTP egress, backend visual correctness, or packaged Story success.

The observer requires decodable image results, exact public payload/saved-byte
agreement, fresh user-accessible artifacts, a distinct edited result, and
correlated canonical same-Thread history selection. Stock media preparation
may resize or re-encode history images; it does not invalidate those artifact
requirements. The [runner contract](../../live/README.md) owns the detailed
correlation, codec, preparation, recovery and resource checks.

The consumed fixture enables only the image tool. A packaged profile may use
the historical view-image-before-edit route; that equivalent path remains
within this retained Story and unproven by C5 scripts. Equivalent notifications
and valid internal recovery do not consume another proof-run invocation.
