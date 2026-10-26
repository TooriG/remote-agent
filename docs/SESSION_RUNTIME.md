# Session runtime

The Host owns execution on the trusted computer. Every client dispatches typed
intents through the Rust Store and renders its immutable state. Crate placement
and dependency rules are in [AGENTS.md](../AGENTS.md).

## History and live execution

Provider-native history is the persistent conversation source. The Host retains
live execution, unresolved requests and unconfirmed input delivery; it does not
maintain a second conversation database or replay log. Completed work is released
when no read or delivery evidence still needs it.

Opening a conversation reads native history and overlays Host-owned live state.
The initial response and subsequent changes use the same authenticated QUIC
stream, so the initial state is applied first. Dropping that stream ends its
subscription. Reopening replaces the subscription; obsolete updates are ignored.
Unavailable history remains explicit and does not erase known live work.

Host and clients exchange shared Rust types through Postcard. JSONL is confined
to external provider stdio. Incompatible shared field or enum layout changes
require updating the ALPN in
[transport.rs](../crates/agent-transport/src/transport.rs).

## Submission and answers

The Host chooses start, resume, steer or queue from current execution state.
Clients never choose a native route from cached status. Session and model
references retain their provider and complete native ID across Rust, storage,
Swift and Kotlin; formatted display keys are not routing authority.

Input IDs correlate pending submissions with native echoes. They suppress
in-flight duplicates within retained evidence, not all duplicates across Host
restarts. Acceptance proves submission, not generation success or durable history.
Completion alone cannot retire evidence for an input that has not been echoed.
Missing evidence never proves non-delivery. Unknown delivery is preserved and
never automatically resent.

The Host owns each public request together with its private native reply source.
It authenticates the connection, validates the target and typed answer, and
claims the request once before native I/O. Concurrent clients cannot both answer.
A complete write proves sending, not provider acknowledgement. Partial or unknown
writes remain unknown; cancellation must not strand a request in Sending or
permit an unsafe retry. Session-targeted requests belong to their issuer, not an
unrelated running turn. Client disconnection leaves execution and valid requests
available to other clients and reconnection.

Provider-specific mappings and the versions used to check them are in
[BEX_PROTOCOL_NATIVE_CONTRACTS.md](BEX_PROTOCOL_NATIVE_CONTRACTS.md).

## Providers, processes and accounts

Adapters translate native history, events, capabilities and answers. Host owns
execution, worktrees, authorization and delivery evidence. Provider failure must
leave other available providers and independent Host resources usable.

Ship `bex-provider-supervisor` with the Host from the same revision. It owns
provider process groups or Windows Job Objects and cleans up child processes on
Host or provider exit. This controls process lifetime; it is not an OS sandbox.

Account selection changes credentials without moving conversation configuration
or history. Claude account/capability probes finish their SDK usage request so
OAuth refreshes can complete without a model prompt; a failed usage read must
not erase a known account identity. Existing turns retain their original account.

## Client state and recovery

Store owns connection recovery and state transitions. Native lifecycle events
activate or pause recovery; individual views do not run retry loops. Independent
operations retain their own targets and generations, so late work cannot replace
newer navigation or input. Overload is explicit and must preserve user work.

[persistence.rs](../crates/agent-core/src/persistence.rs) defines the device storage
contract. User-owned state is scoped to Host identity and provider storage area;
returning to an area restores its drafts and unsaved edits. Runtime history,
catalogs, connection authority and operation progress are fetched again.
Resource bounds live beside their owning implementations and tests.
