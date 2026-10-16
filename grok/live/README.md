# Explicit-binary Live scenarios

See [TESTING](../TESTING.md) for the default offline, tagged Integration,
opt-in Live and independent Facts commands. This package owns business
observations; the [Go catalog](../internal/scenarios/catalog.go) owns invocation
selection, eligibility and budgets. The [Story index](../docs/stories/README.md)
links every retained guarantee to its actual Go or Rust owner.

`Basic(ctx, Options)` uses the pinned `llm-go/codexsdk` client to consume the
selected binary's App Server v2 stdio API:
initialize, one fixture-bound thread, one text turn, matching completion and a
nonempty final reply. Primary/pinned fixtures use `internal/providerfixture`
controls and an isolated text-only profile/catalog, independent of `grok/dist`.

Supply an absolute binary path and expected SHA-256, caller-established source,
harness/target/environment metadata, model, Provider base URL and key. The binary
must remain stable during a direct invocation of these functions. The canonical
Integration and Live/CLI entry points instead prepare a byte-verified owned
execution snapshot, preserving package-relative helpers and original subject
metadata; see [the snapshot boundary](../TESTING.md#integration-actual-codex-and-controlled-grok-http).
A deadline of at most ten minutes
is required. The runner never restarts/resubmits; product retries retain their
policy. SDK frames cap at 16 MiB; Basic's completion callback queue caps at 128
notifications/8 MiB and SDK retention at 512 slots/8 MiB. Basic uses explicit
`ClientOptions.Env`, `NewContext`, typed thread/turn calls and `CloseContext`
with a separate five-second cleanup deadline. Other scenarios retain their
application-owned evidence and retention bounds described below, with SDK-owned
process/transport cleanup. `CleanupPending` explicitly marks an unproved join;
fixtures remain available on that path instead of being removed under a child
or admitted callback. Constructor failure exposes no SDK join handle.

Returned evidence contains safe declared metadata, time, counts, booleans,
reply byte count and last stage. Errors contain no private traffic; failure
attribution remains inconclusive without independent evidence. Success observes
only this path; source/target provenance and native eligibility are prerequisites
for a Story claim, supplied by callers and #339's process integration.

CI tests the public stdio boundary deterministically and separately activates
`TestIntegration/BasicFixture` with `-tags=integration` and its freshly built CLI. Controlled HTTP proves
config/request/stream composition, not real backend Live. Actual execution and
invocation integration remain #331/#339.

## Fixed-input SDK migration

The [ordered migration](https://github.com/Harness-X-Harness/codex/issues/379)
fixes product source `e9268d5211064fb77f76a9fe7f0a99348488c9d2` and the owner-approved SDK revision
`8ade447507bc3d08650b830ee6306dd27bc17f23` (`v0.2.1-0.20261014071218-8ade447507bc`),
which supersedes `9769ba362a5e946c17a6eb78d8e2155e19bb4f28`. Direct facades no longer
arm an unowned attachment fence. The module uses Go 1.26.8. The verified Linux musl x64 product executable from canonical
run `38171472745`, attempt 1, artifact `11714608891` has SHA-256
`26ee0bce9bbcb4817d87b99a31a75367e5dfe142d5e9e1f2cc93468610faf1ab`.
Use [explicit verified package selection](../package/ARTIFACTS.md); verify
expiration and exact subject on retrieval. `runtime_sha256` identifies the
executable, separately from the artifact ZIP or package archive hash. The
product source remains fixed while the declared harness SHA evolves.

Basic (#383), the formal/exploratory reasoning controls (#384), the four
structured-edit entries (#385), image generation/edit (#386), the shipped
catalog, startup, pinned-history, and child entries (#387), and hosted Web/X
search (#388) use the SDK. The handwritten stdio client is gone. SDK failures
never switch clients. Real-provider Live remains outside this fixture proof.
Shared fixture setup verifies the selected binary and isolates home/credentials.
Completion needs independently matching thread/turn identities, a successful
terminal and a nonempty eligible final reply; SDK success alone is insufficient.
Malformed action requests may be wire-rejected without invalidating that result;
valid unauthorized requests are denied without effect. The reviewed test map
keeps identity, outcome, authorization, bounds, secrecy and invocation contracts,
replaces old request-abort controls with rejection/non-execution coverage, and
retires exact legacy error/stage/code expectations. SDK defect
[llm-go #422](https://github.com/ronhuafeng/llm-go/issues/422) remains separately
tracked and nonblocking under #383's explicit contract decision. Native fixtures
and ordinary CI builds do not establish real backend Live or frozen-byte proof.
The typed stream attachment, independent identity checks and inline-response
snapshot also account for separately tracked SDK [#423](https://github.com/ronhuafeng/llm-go/issues/423),
[#424](https://github.com/ronhuafeng/llm-go/issues/424) and [#425](https://github.com/ronhuafeng/llm-go/issues/425).

`ReasoningHistory(ctx, Options)` shares Basic's artifact/process/fixture binding
and adds the [encrypted-history Story](../docs/stories/grok-encrypted-reasoning-history-continuation.md).
It opts into raw events and one named dynamic tool, with explicit medium effort.
The SDK owns stream attachment and same-thread resume; resume excludes returned
turn history and retains the original model, Provider and fixture cwd. The
application checks both thread/turn identities independently, including valid
requests that arrive before the turn-start response. SDK retention caps at
65536 slots/64 MiB; provisional completion evidence caps at 8192 entries/
8 MiB. Accepted tool identities retain at most 4096 SHA-256 digests, and only
those identities can establish completed tool-result evidence. A repeated call
at capacity remains allowed; a new identity at capacity receives a token-free
denial and a static failure. These bounds are independent of SDK retention.
Typed callbacks observe item and turn completion; the validated raw extension
observes opaque encrypted reasoning. Successful history-cursor completion fences
the SDK callbacks before proof, independently of the application Story oracle.
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
secret-safe failures and invocation budgets for both model controls.
`TestIntegration/ReasoningHistoryFixture` uses the selected product against controlled
HTTP: N returns opaque reasoning plus a named tool call, the next product
request carries its fresh paired result, and N+1 replays the unchanged encrypted
history with a token-free user input on the same thread. It checks exactly three
HTTP requests and excludes plaintext reasoning from replay. The fixture supplies
the answers, so this proves product/SDK composition and cannot establish real
backend recall or Story PROVEN. CI uses its freshly built product; explicit
frozen-package runs separately verify the pinned executable bytes.

#384's scripted-test review inventory retains all 118 original modes for both
models and adds five focused identity controls. The semantic map is:

| Decision | Controls | Continuing obligation / replacement |
| --- | --- | --- |
| KEEP | 110 original modes | Binding, fresh authorized tool result, encrypted completion, matching output, one token-free continuation, eligible recall, terminal/partial failure, replay reconciliation, assistant bounds and secrecy. |
| REPLACE | `unsupported`, `early_unsupported` | A valid approval request receives a token-free denial without effect; an exact legacy RPC error is unnecessary. |
| REPLACE | `no_reason` | Schema-valid non-reasoning raw traffic and a reasoning delta cannot prove completed reasoning. |
| REPLACE | Four `identity_missing_item` / `identity_missing_summary` first/next controls | SDK decoding failure cannot fabricate terminal/recall evidence; a continuation failure preserves proven N. |
| REPLACE | `request_turn` | Either a known-ID mismatch receives a token-free refusal, or an early provisional response fails actual-ID reconciliation before any terminal, completed-tool or recall credit. The truthful response count remains; concurrent SDK callback timing cannot define authority. |
| ADD | `tool_wrong_call`, `tool_identity_limit`, `tool_identity_overflow` | Completed tool evidence must match an accepted call; 4096 distinct calls and a consistent retry are allowed, and overflow receives a token-free denial with a static failure. |
| ADD | `cipher_missing_identity`, `cipher_empty_identity` | Nonempty encrypted reasoning without an item identity cannot establish encrypted evidence or authorize N+1; truthful completed-tool and final-reply partial facts remain. |

No original mode is retired. Universal assertion changes replace incidental EOF
and stage timing with bounded failure, truthful completion/recall facts and safe
classification. Malformed transport remains a boundary/secrecy control rather
than an independent positive Story requirement.

`StructuredEdit`, `StructuredEditApprovalDeclined`,
`StructuredEditPinnedPreviousModel` and `StructuredEditReplaceAll` use the shared
runner with a capability-enabled seeded workspace. Their effectful stdio tests
prove deterministic support only. The [structured-edit Story](../docs/stories/grok-structured-edit.md)
owns acceptance, invocation budgets, downstream prerequisites and retention.

#385's fixture preparation preserves all 66 scripted modes (11 positive and
55 negative) while making their modeled responses conform to the pinned public
SDK schema. Public edit dispatch uses the shared typed SDK runner. `TestIntegration/StructuredEditFixture` exercises the four edit
entries against controlled HTTP: primary continuation, pinned, declined approval
and replace-all. It checks actual bytes/hashes and matching call/output history;
the supplied replies and HTTP acceptance do not establish real-provider Live.
The required native selection includes this fixture and fails if it is absent.

The four edit entries use direct `Threads().Start`, `Turns().Start` and
`Threads().Read` with the private observer. Callbacks must admit the terminal
before Read; Read validates both retained turns and does not synthesize a missing
terminal. Actual file bytes and SHA-256 are checked after each turn and again
after `CloseContext` joins callbacks and reaps the child. A matching file-change
request on a non-decline fixture is declined and becomes a sticky failure, so a
later write cannot become an authorized edit. Identical call, output and approval
replays stay one semantic event; conflicting identities still fail. The scripted
map keeps 60 outcomes, replaces `duplicate_call`, `duplicate_output`,
`stale_continuation`, `approval_thread`, `approval_turn` and `duplicate_approval`,
and retires none. Transport stage strings and the old per-field correlation codes
collapse to `edit_evidence_unpaired` where the observer has one causal proof.

`StructuredEditEvidence` returns counts, fixture hashes and separate first/current
terminal facts (`FirstCompleted`, `TurnCompleted`); `Completed` requires the whole
scenario. `BytesMatch` and `AfterSHA256` describe the current turn's comparison
and reset when a turn begins. A later failure preserves observed terminal facts
without reporting overall completion. The observer uses one settled thread read
per turn, at most 4096 preterminal frames, 4096 argument bytes, 64 KiB per output,
and one in-memory call/output/change correlation. It collects no outbound HTTP
request or raw session artifact.

`ImageGenerationEdit(ctx, Options)` uses direct `Threads().Start`, `Turns().Start`
and `Threads().Read` on one image-capable fixture: one natural generation turn,
then one natural edit turn on that thread. Callbacks admit canonical raw items
and public image terminals before Read; Read does not synthesize a missing
terminal. `TestGrokImageGenerationEdit` executes deterministic fixture scripts
for primary/pinned models. This is prerequisite support for the
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
(at least four bytes per pixel, eight for 16-bit RGBA). The SDK inbound frame
limit remains 16 MiB, and scenario observation stays at 4096 frames/64 MiB,
independent of the SDK retention budget. A result exceeding an observation
budget is not proven by this harness; it is not evidence that the product
rejected a valid image. Failures preserve safe partial terminal/artifact facts; returned
evidence omits prompts, payloads, paths, credentials and IDs.

C6 adds source-backed `ShippedCatalog`, `ShippedStartup`, `ShippedPinned` and
`ShippedChildCollaboration`. These copy the exact embedded `grok/dist` profile
and catalog, verify the entire public Model DTO response, and keep the existing
explicit binary/digest/deadline boundary. Catalog-only proof makes no inference
request. Startup keeps the profile's default effort; collaboration requests Ultra.
The pinned scenario retains the two-turn named-tool/encrypted-history oracle.

The shipped child observer submits one bounded seed setup turn and then one
original delegation task. It records separate setup/task budgets and requires a
qualifying child to recall the setup's fresh 128-bit marker without receiving it
again in its own inputs or through a tool lookup. Stock paginated child history
intentionally omits inherited parent records, so exact child public seed-prefix
presentation is not required. The controlled HTTP prerequisite still checks the
complete inherited seed content and order. The observer binds a fresh completed
child result to a parent terminal reply containing it. Parent prose,
incidental failed/running children and later child orchestration are permitted
without being credited. Missing, wrong or newly supplied setup markers, inherited
results, pre-spawn parent-known nonces, wrong Provider/model/lineage and observed
terminal conflicts are rejected. Hexadecimal value matching ignores letter case. No
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


## C7 hosted-search scenario support

`HostedSearch(ctx, Options, scenario)` retains `web`, `web_allowed`,
`web_excluded`, `x`, and `x_window` using explicit Grok fixture binding and
public `thread/start` configuration overrides. The fixed policy controls match
`facts.SearchPolicy`; the fixture enables hosted Web/X and keeps local execution
disabled. The positive search contract starts one search turn and one
same-thread follow-up, each once,
with one settled `thread/read` per completed turn. Failed or incomplete first
turns never trigger a follow-up. Product-internal transport attempts are not
constrained by this runner's semantic invocation budget.

The observer requires supported completed canonical hosted items from matching
`rawResponseItem/completed` notifications, both terminal turns, and authoritative
nonempty final replies in the settled thread. Consistent duplicate hosted items
count once; conflicting duplicates, unrelated-only evidence, local calls/outputs,
missing calls, missing final text and failed follow-up cannot complete it. The
public raw notification follows canonical history recording in the current
product source. This observation is not an independent durable-file inspection.
Per turn, observation is bounded to 4096 frames / 64 MiB and 64 hosted calls of
at most 64 KiB each, in addition to the existing stdio bounds. Credited identities
are keyed by turn and item and retained across the single continuation, at most
128 total. Consistent delayed prior-turn duplicates do not add credit; changed
payloads or newly appearing hosted items after that turn's settled-read fence
fail. Supported hosted X may coexist with a Web scenario without being mistaken
for local custom execution; only the selected scenario earns hosted credit. Only safe metadata,
counts, booleans and last stage leave the observer.

Deterministic public-stdio fake-process tests execute each retained policy,
exercise successful and failed first/follow-up turns, validate exact config
requests, and prohibit runner resubmission. They enter the shared default
`go test -race -count=1 -timeout=15m ./...` job automatically. The runtime
job executes only tagged `TestIntegration*` owners after its one CLI build.

This is deliberately narrower than the historical packaged hosted Stories:
HTTP wire replay and policy projection require current native composition proof;
actual packaged/backend execution requires #331/#339 and a reviewed invocation.
This observer does not establish outbound replay, cold restart, search-result
relevance, UI streaming completeness or temporal lifecycle behavior (#362/#363).
No old Story is reintroduced as accepted authority and no Live PROVEN result is
claimed. The retained source and offline tests cannot close those independent
obligations.

## Original-wire SDK boundary

The original App Server stdout is delivered to the SDK unchanged. No public
`xSearch` notification is dropped to make a hosted test pass. The current pinned
SDK rejects that unsupported public variant. The owner-approved
[#408](https://github.com/Harness-X-Harness/codex/issues/408) contract accepts
that exact rejection for `x`/`x_window` only, provided the public SDK shutdown
result supplies the actual decode cause, cleanup is confirmed, and observed or
SDK-retained business contradictions, artifact failures and timeouts still fail. A generic protocol end or other unknown
variant cannot satisfy it. The SDK pin, stock schema and validation stay strict.
On fatal decoding, the SDK may discard undelivered raw callbacks. Zero pending
callbacks proves cleanup, not that every callback executed. Expected rejection
therefore proves the exact SDK rejection, not complete raw delivery or an audit
of all raw payloads; observed and SDK-retained contradictions remain disqualifying.

`SearchEvidence.ExpectedUnsupported` distinguishes accepted rejection from
positive search: completion flags stay false, no follow-up or settled turn is
credited, and canonical counts reflect only observations actually received.
The CLI reports `expected_unsupported` with safe code `sdk_xsearch_unsupported`;
raw errors and private notification contents are not retained. Raw canonical
hosted evidence and public activity remain distinct oracles: a public-only
notification earns zero canonical, settled or completion credit. Web and other
supported paths retain the complete positive contract above, including when
mixed Web/X evidence is tested. Fake-process rejection tests establish only
the controlled SDK boundary; fresh actual-binary Integration evidence is still
required. This scoped acceptance change does not establish positive X SDK
support or complete the retained #388/#379 hosted Story obligations.

The historical SDK/product package identifiers above describe a previous fixed
subject. They are not evidence that this worktree, a newly compiled product or
a different platform has passed native or real-backend acceptance.
