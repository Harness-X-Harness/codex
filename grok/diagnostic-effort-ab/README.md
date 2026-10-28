# Temporary prompt A/B diagnostic — never merge

Dispatch-only experiment branch; no formal Live oracle or version branch changes.
This revision tests only the first user prompt phrase. A says `reason about its
returned token`; B says `copy its returned token`. All other words, model, tool,
strict token schema, profile and permissions stay unchanged. Both arms omit effort.
The model is grok-4.6. Fixed binary artifact 12769806175 has runtime SHA-256
47de14909531f35f7eacee71c1040bcb5d228ca1405ea2935327efef549e4c16;
harness is e3c0812c7d47c567d42a181c921a30aa3c35cdc0.

Exactly A then B, at most two HTTP requests each and four total; no reverse pair,
automatic retries or redirects. Reservations occur before sending; errors never
refund budget. Second sends must be paired tool results, not initial-request
retries. Unknown failures, timeout or incomplete cleanup stop the experiment.
A 403 is an observed response, not proof of a particular internal policy.
Do not rerun or start another dispatch without authorization.

Before the backend secret is available, CI validates the fixed package and uses
the actual binary against a synthetic loopback server. Both arms and the separate
403/zero-retry check run before aggregation. The gate requires the exact prompt
variant, effort absent and all other request fields equal after explicit fresh
identity normalization: session cache key, temporary cwd, random tool token, and
runtime message/msg_UUID or function_call_output/fco_UUID identities. ID mappings
are per arm and shared across both requests, preserving replay identity relations.
Call IDs, server IDs, arbitrary unknown IDs, cipher and all other content are not
normalized. Only the exact B user-message text maps to A for comparison; outgoing
requests are never rewritten. Any unknown difference prevents real execution.

Safe artifacts include all arm outcomes, cleanup, initial/continuation shapes,
UTC/status/request IDs, schema digest, cipher presence/length and comparison JSON
paths/types/lengths/hashes/structural markers. No credential, request text, private
reasoning or ciphertext is stored. Offline reports are written even on failure.
Only exact runtime/source-bound offline proof permits the subsequent secret step.
The relay buffers SSE with a 90-second socket timeout: these shared experimental
transport changes limit extrapolation and a transport failure is not a model 403.

Prior effort experiment 40610902144 completed its offline gate and sent exactly
two real requests: unset and high both received initial HTTP403. Its two earlier
preparation runs made zero real requests. This prompt experiment is separately
authorized for at most four requests; unused old budget is not silently reused.
The branch reuses the registered workflow path but must never be merged.
