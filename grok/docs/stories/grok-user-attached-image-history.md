# Grok user-attached image conversation

## User story and boundary

As a Grok user, I can attach a picture to a conversation, receive an answer
about its visible content, then ask about a different part of that picture in
the same Thread without reattaching it. The boundary is the exact selected
Codex App Server binary, its bound Grok provider, and the pinned image-capable
model. This is user image input, not generation or the image-edit tool.

## Acceptance

Given an identified binary and the isolated image-input-capable, tool-disabled
fixture, the llm-go SDK starts one Grok Thread. The first Turn submits an actual
UserInput.Image with a deterministic PNG and asks for only the left object's
color and shape. The second Turn contains only text and asks for the right
object's color and shape from the earlier picture.

Both distinct Turns must have matching successful terminal evidence and
successful bound history readback. The first accepted final answer contains
only the left fact, and public assistant commentary must not add other facts,
so that public answer text cannot supply the second answer. The original inline
attachment remains in the first user message in public Thread history after
both Turns. Answers are compared as semantic enums with case, whitespace and
punctuation normalization, never substring matching. Prompts and filenames do
not disclose either answer; the picture pixels carry the facts.

Partial success, wrong-thread terminals, failed terminals, absent attachments,
missing retained image, duplicate Turn identity, a nonvisual generic answer,
and a first answer that reveals the second fact do not complete this Story.
An unavailable answer is not replaced by another invocation. Product-internal
retry policy and the existing scenario deadline remain unchanged.

## Proof plan and evidence limits

- One `attached-image-pinned` full-suite scenario, using grok-4.6, one process,
  one Thread, two semantic Turn submissions, no runner retries or variants.
- Deterministic prerequisites: pixel and prompt checks, actual typed SDK input
  checks, success and false-acceptance cases in
  [the offline observer tests](../../live/attached_image_script_test.go).
- [The controlled binary fixture](../../tests/integration/attached_image_test.go)
  requires the existing exact selected binary/snapshot, validates inline image
  provider input on both requests and returns controlled answers. It does not
  prove real-provider visual understanding.
- The existing same-run shipped-binary prerequisite and full-suite aggregation
  are unchanged; the original seventeen scenarios remain required.
- Retained evidence contains only subject metadata, counts, stages, booleans,
  and reply lengths. `FirstRecall` means the first visual answer matched;
  `Recalled` means the distinct second visual answer matched, not simple recall
  of the prior assistant's text. No image, prompt, reply, private identifier,
  credentials or raw traffic is exported.
- A separately authorized identified real-backend run is required to establish
  the user-visible result. Offline checks do not establish Live acceptance.
  This Story does not prove arbitrary images, other models, generation/edit,
  cold restart, compaction, latency or absence of product-internal attempts.

Opaque/internal reasoning may retain visual observations. The outcome proves a
usable image-bearing same-thread continuation, not that the second backend
request necessarily re-reads pixels instead of using those internal observations.
