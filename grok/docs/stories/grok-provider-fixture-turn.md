# A configured Grok fixture completes a text turn

## User story and real path

As a user of an explicitly selected Codex binary, I can select the configured
Grok Provider and model and receive a completed ordinary text turn through
App Server v2: initialize -> thread/start -> turn/start -> visible completion.

## Acceptance

Given an exact binary with the relevant Provider/tool native proof, credentials,
an isolated harness-owned profile/catalog fixture, and a usable default execution
environment, when the runner initializes App Server, creates one thread, and
submits one ordinary text input, then the returned Provider/model match that
fixture and that turn completes with a nonempty final assistant message.

The fixture is independent of `grok/dist`. It does not establish shipped catalog
contents, product defaults, another model, or another binary's behavior.

## Partial success and failure boundaries

Process startup, thread creation, text deltas, and a self-reported Provider name
are insufficient without the same operation's completed turn and final reply.
Another Provider, thread, or turn cannot supply the result. The runner does not
resubmit a failed or incomplete operation. Product-internal same-Provider retries
retain their own policy; this Story does not promise one HTTP attempt.

## Proof plan

- Require reviewed native Provider/config/request/stream and harness proof for
  the source being tested; identify the actual binary by its expected digest,
  source/target metadata, harness revision, environment and observation time.
- Allow one process initialization, one thread creation and one semantic turn
  submission. Setup cannot be repeated to replace a failed run.
- Accept a final assistant message carried by a completed item or by the
  completed turn's items, with causally matching thread/turn identity. Public
  App Server representation compatibility is a separate prerequisite; one
  incidental envelope/order is not the only semantic oracle.
- Retain only safe subject metadata, completion/binding booleans, invocation
  counts, reply byte count and last proven stage. Do not retain raw traffic,
  credentials, prompt/reply text, opaque reasoning, or thread/turn identifiers.
- Missing completion means NOT_PROVEN. Attribute a failure only when bounded
  evidence identifies its boundary; otherwise use inconclusive, without an
  automatic rerun or product repair.

Real Live is not run by required CI. A backend opt-in skip is not PROVEN.
