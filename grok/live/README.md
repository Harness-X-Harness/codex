# Explicit-binary Live scenarios

`Basic(ctx, Options)` consumes the selected binary's App Server v2 stdio API:
initialize, one fixture-bound thread, one text turn, matching completion and a
nonempty final reply. Primary/pinned fixtures use `internal/providerfixture`
controls and an isolated text-only profile/catalog, independent of `grok/dist`.

Supply an absolute binary path and expected SHA-256, caller-established source,
harness/target/environment metadata, model, Provider base URL and key. The binary
must remain immutable during the invocation. A deadline of at most ten minutes
is required. The runner never restarts/resubmits; product retries retain their
policy. Frames cap at 16 MiB, early traffic at 128 frames/8 MiB. Cleanup closes
stdin, allows bounded shutdown and stops/reaps the child when needed.

Returned evidence contains safe declared metadata, time, counts, booleans,
reply byte count and last stage. Errors contain no private traffic; failure
attribution remains inconclusive without independent evidence. Success observes
only this path; source/target provenance and native eligibility are prerequisites
for a Story claim, supplied by callers and #339's process integration.

CI tests the public stdio boundary deterministically and activates
`TestNativeBasicFixture` with its freshly built CLI. Controlled HTTP proves
config/request/stream composition, not real backend Live. Actual execution and
invocation integration remain #331/#339.

`ReasoningHistory(ctx, Options)` shares Basic's artifact/process/fixture binding
and adds the [encrypted-history Story](../docs/stories/grok-encrypted-reasoning-history-continuation.md).
It opts into raw events and one named dynamic tool, with explicit medium effort.
Only a valid first-turn tool request creates the in-memory 32-byte random token;
repeated calls return the same token. Completed reasoning, opaque encrypted
output, a successful completed named tool and final token recall must all match
turn N before a single fixed, token-free continuation is submitted. N+1 must
complete and recall that token; tool requests receive a token-free denial while
observation continues. No private token, ciphertext, text or identity is returned.

Per turn, reply evidence retains at most 4096 assistant-ID SHA-256 digests and
final-eligibility/recall bits. Consistent duplicates cannot revive a superseded
reply; conflicting duplicates prevent proof until an authoritative terminal
assistant summary reconciles them. A previously unseen delayed final remains
eligible when that summary is absent. Missing assistant IDs and identity-budget
overflow fail with static errors. This is a harness memory bound, not a limit on
product tool calls or retries. Authoritative summaries bypass and release the
digest map; provisional evidence starts fresh for the continuation.

Deterministic public-stdio tests cover ordering, correlation, terminal evidence,
secret-safe failures and invocation budgets for both model controls. Actual
native tool/HTTP composition and real-provider execution require #322; the
current product rejects nonempty Grok tools. This harness increment does not
establish backend success, unchanged native encrypted replay, or Story PROVEN.

`StructuredEdit`, `StructuredEditApprovalDeclined`,
`StructuredEditPinnedPreviousModel` and `StructuredEditReplaceAll` use the shared
runner with a capability-enabled seeded workspace. Their effectful stdio tests
prove deterministic support only. The [structured-edit Story](../docs/stories/grok-structured-edit.md)
owns acceptance, invocation budgets, downstream prerequisites and retention.

`StructuredEditEvidence` returns counts, fixture hashes and separate first/current
terminal facts (`FirstCompleted`, `TurnCompleted`); `Completed` requires the whole
scenario. `BytesMatch` and `AfterSHA256` describe the current turn's comparison
and reset when a turn begins. A later failure preserves observed terminal facts
without reporting overall completion. The observer uses one settled thread read
per turn, at most 4096 preterminal frames, 4096 argument bytes, 64 KiB per output,
and one in-memory call/output/change correlation. It collects no outbound HTTP
request or raw session artifact.

`ImageGenerationEdit(ctx, Options)` consumes an image-capable fixture through
the same bounded runner: one natural generation turn, then one natural edit
turn on that thread. `TestGrokImageGenerationEdit` executes deterministic public
stdio scripts for primary/pinned fixtures. This is prerequisite support for the
[retained image Story](../docs/stories/grok-image-generation-history-edit.md),
not actual Grok tool/HTTP/backend or packaged execution.

The observer reconciles each canonical raw call and image output with the
matching completed public image item and settled `thread/read`. The previous
image must be selected by the exact latest-image window or its exact saved
path; later edits may continue the same verified lineage. Missing, stale,
unrelated or conflicting evidence cannot establish completion. Consistent
duplicate terminals and a failed tool call followed by a successful internal
recovery are supported; tool multiplicity is diagnostic.
Validation or image-file resolution can fail before an image lifecycle item
starts. An exact image call paired with a nonempty text error and no image item
remains diagnostic. It cannot establish image lineage or completion, and an
invalid invocation cannot supply a successful image. A later valid image call
must satisfy every normal image, artifact, history and terminal requirement.
This follows `ext/image-generation/src/tool.rs::handle_call` validating before
`emit_started`, and `core/src/tools/parallel.rs::failure_response` returning a
same-call text output on failure. A started image still requires its terminal
item; an incomplete lifecycle cannot use this diagnostic exception.

This fixture explicitly disables view-image, shell, Code Mode, apps and the
other tool features. Only the image tool is enabled, and explicit Grok
`transparent_background` arguments are unavailable. The observer rejects tools
outside this fixture's surface. That isolation does not narrow the retained
packaged Story: a general profile's view-image-before-edit path remains an
unproven packaged obligation, not a result established by these scripts.

The source chain is `core/src/session/mod.rs::record_conversation_items` (history
media preparation, history recording, rollout persistence, then raw events), `core/src/tools/handlers/
extension_tools.rs` (canonical history passed into `ToolCall`), and
`ext/image-generation/src/tool.rs::{recent_images,GeneratedImageOutput}`
(newest-first selection and PNG output). App Server's
`request_processors/thread_processor.rs::{read_thread_view,load_live_thread_view}`
reads the exact loaded thread's history; its `extensions.rs` binds the image
save root to the isolated home. No broad durable-session scan is used.

Stock `core/src/image_preparation.rs` may resize/re-encode the image before
history recording. The prepared history rendition and public artifact therefore
retain separate digests. The exact call ID binds the canonical rendition to its
public image, and the later selector binds that rendition to the edit. Both
renditions must fully decode. Public payload/file equality remains strict;
prepared/full-resolution byte equality is not an oracle. The controlled script
includes a 2048-square PNG prepared as 1600-square PNG. Native C5 image
preparation tests remain required to prove the real transformation owner.

PNG representation is checked by full codec decode, bounded dimensions, and
exact payload/file equality. Artifacts must be new under the controlled image
root, nonsymlink regular files, remain readable, and the edited result must
differ. A PNG header alone and JPEG mislabeled PNG fail. Each turn caps at
4096 frames/64 MiB; the scenario retains at most 64 call records, inspects at
most 256 artifact entries per pre-turn inventory, and arguments to 4096 bytes.
Image encoded bytes and decoded pixels use the extension's 32 MiB local budget
(at least four bytes per pixel, eight for 16-bit RGBA). The inherited 16 MiB
stdio frame and 128-frame/8 MiB early queue budgets still apply, including to
settled multi-turn reads. A result exceeding an observation budget is not
proven by this harness; it is not evidence that the product rejected a valid
image. Failures preserve safe partial terminal/artifact facts; returned
evidence omits prompts, payloads, paths, credentials and IDs.

C6 adds source-backed `ShippedCatalog`, `ShippedStartup`, `ShippedPinned` and
`ShippedChildCollaboration`. These copy the exact embedded `grok/dist` profile
and catalog, verify the entire public Model DTO response, and keep the existing
explicit binary/digest/deadline boundary. Catalog-only proof makes no inference
request. Startup keeps the profile's default effort; collaboration requests Ultra.
The pinned scenario retains the two-turn named-tool/encrypted-history oracle.

The shipped child observer submits one bounded seed setup turn and then one
original delegation task. It records separate setup/task budgets, verifies the
actual public seed prefix in a qualifying child's history, and binds a fresh
completed child result to a parent terminal reply containing it. Parent prose,
incidental failed/running children and later child orchestration are permitted
without being credited. Partial/altered/unrelated history, inherited results,
wrong Provider/model/lineage and observed terminal conflicts are rejected. No
setup or task is resubmitted. Only safe counts/booleans/stages leave the runner;
setup text, result UUIDs and thread IDs do not.

Child discovery uses actual public V2 started activity and matching child paths.
The existing parent raw-event opt-in is inherited by child listeners. The observer
retains at most 512 relevant notifications / 1 MiB in memory and requires ordered
consumed `AgentMessage` inputs, raw result text and matching public child history
through the credited completed turn. Pre-result supplied nonces and exact credited
terminal conflicts prevent credit; later mentions and unrelated activity do not.
Earlier failed own turns need consistent complete input/lifecycle evidence, not
success. Missing notifications after a durable read are awaited within the original
deadline and event bound; contradictions settle immediately, without another RPC.
Absent/partial raw coverage, early closure or opaque pre-result input is
NOT_PROVEN. No raw content is persisted or returned. The native listener fixture
uses a real stock Responses V2 child with the existing plaintext tool marker and
checks opt-in/out inheritance without subscribing to the child separately.

C6's HTTP lifecycle fixtures explicitly disable tools in test copies. Shipped
assets retain every capability. Full shipped tool/media/history wire composition
remains C7, package invocation #339 and actual backend Live #331. Deterministic
scripts and native catalog checks do not establish those outcomes.
