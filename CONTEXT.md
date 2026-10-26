# Terminology

- **Host**: the trusted computer-side daemon that owns agent processes and
  filesystem access. Its management operations are restricted to local clients.
- **Client**: the desktop, iOS, Android or headless application connected to a Host.
- **Paired device**: a client identity authorized by the Host. Pairing grants access
  to files available to the Host account; revocation removes future access.
- **Host identity**: the long-lived iroh public identity, distinct from an address
  or an expiring invitation. Client data is also scoped to the provider storage area.
- **Conversation / session**: provider-native history and its current execution,
  referenced by the provider and complete native ID.
- **Turn / item**: one unit of agent work and an individual message or activity
  within it. An input receipt is not proof that a turn completed.
- **Steering input**: additional input to an active turn, when supported by its
  provider. The Host chooses the submission route.
- **Snapshot**: Store's immutable runtime view. It is not the device storage format.
- **Draft / file draft**: client-owned unsent input or unsaved file edits. File saves
  check the Host revision; draft recovery preserves newer user edits.
- **Pending submission**: sent content retained until delivery can be reconciled.
  Unknown delivery is not automatically resent.

See [Session runtime](docs/SESSION_RUNTIME.md) for ownership and recovery.
