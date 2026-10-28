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
successful bound history readback. The first accepted final answer supplies
the left fact without disclosing the second fact. Public assistant commentary
may describe the work; explicit disclosure of either protected second-fact
property would let the continuation repeat prose instead of using image context
and cannot establish this proof.

Each Turn carries a formal SDK output schema: an object with required `color`
and `shape` properties, each string or null, and no additional properties.
No expected color/shape is embedded in a schema enum, const, prompt or filename;
the picture pixels carry the facts. The bounded strict JSON parser rejects
malformed, duplicate-key, missing-field, extra-field and wrong-type responses.
Null means unknown and does not pass. String facts are compared to the actual
fixture after case/whitespace normalization, preserving the existing
`circular`/`circle` equivalence rather than matching prose. The prompt requests
a simple shape noun without disclosing any expected shape.

The actual image is supplied once through UserInput.Image. Correct answers in
the same conversation establish usable image context; no exact data URL or
internal image representation in history is an additional acceptance condition.
Missing the initial image input, a wrong visual answer, failed or wrong-thread
terminals, duplicate Turn identity, and explicit second-fact disclosure in the
first reply do not complete this Story.
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
  provider input and formal schema on both requests and returns controlled JSON answers. It does not
  prove real-provider visual understanding.
- The existing same-run shipped-binary prerequisite and full-suite aggregation
  are unchanged; the original seventeen scenarios remain required.
- Retained evidence contains only subject metadata, counts, stages, booleans,
  and reply lengths. `FirstRecall` means the first visual answer matched;
  `Recalled` means the distinct requested second fact matched in the same
  image-bearing Thread; it does not identify which internal or public
  representation supplied that answer. No image, prompt, reply, private identifier,
  credentials or raw traffic is exported in the aggregate. A separate bounded
  diagnostic bundle preserves the synthetic input image, prompts/schema, public
  replies, parse/semantic outcomes and failure evidence; credentials, raw traffic
  and opaque reasoning remain excluded.
- A separately authorized identified real-backend run is required to establish
  the user-visible result. Offline checks do not establish Live acceptance.
  This Story does not prove arbitrary images, other models, generation/edit,
  cold restart, compaction, latency or absence of product-internal attempts.

The public-message isolation check is not a general natural-language judge:
it rejects either second-fact token across all public replies and commentary,
but does not establish the
absence of every possible paraphrase or coded disclosure. A pass must not be
reported as proof of universal non-leakage or pixel-only recall. The controlled
negative cases include a leaked full answer and each separately leaked property.

Opaque/internal reasoning may retain visual observations. The outcome proves a
usable image-bearing same-thread continuation, not that the second backend
request necessarily re-reads pixels instead of using those internal observations.
