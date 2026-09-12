# Grok image generation and same-Thread history edit

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

**Given** an exact packaged Grok artifact whose source, target, checksum, and
Grok Provider profile match the deterministic prerequisite gates,

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

- Exact product source, target triple, and archive checksum.
- Native Grok image, catalog, projection, codec, MIME, stock history-edit, App
  Server lifecycle, and stock Provider-control tests passed for that source.
- The release-bundled image-capable model and the supported Grok Provider
  profile.

### Proof-run invocation budget

One generation Turn and one later edit Turn in the same Thread. The runner
does not retry or replace either semantic Turn.

### Secret-safe evidence

Record exact source SHA, target triple, archive identity and checksum,
Provider and model labels, generation and history-edit completion booleans,
same-Thread and history-argument verification booleans, image MIME/codec match
booleans, and runner submission count. Negative evidence: no runner
resubmission, no alternate Provider, no raw names, arguments, prompts,
replies, IDs, credentials, raw traffic, paths, or image payloads.

## Stock compatibility control

The exact deterministic gate runs the pre-existing stock `ImagesClient`
generation and edit request/response regressions at the changed shared client
seam. Provider catalog, capability, flat projection, image dialect, and
effective edit budget checks remain Grok-specific. The `image_gen.imagegen`
tool identity and App Server item-shape compatibility remain a separate
deterministic child proof; they are not the unique Live image-result oracle.

## Executable contract

`TestGrokImageGenerationEdit` in the repository-owned `grok/live` Go module.
Normal Grok delivery runs
`go test ./... -count=1 -timeout 30m -run '^TestGrok'` from `grok/live` against
the exact current-run Linux archive. `llm-go/codexsdk` is the SDK dependency,
not the acceptance owner.
