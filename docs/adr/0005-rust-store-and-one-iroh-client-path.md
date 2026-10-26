# Rust Store and one iroh client path

Desktop, SwiftUI, Compose and the headless CLI share a Rust Store and immutable
snapshots. One state owner keeps workflows and conversation presentation
consistent across clients; native adapters retain rendering, input and OS APIs.

All clients use the same authenticated iroh transport, including desktop on the
Host's computer. Separate request, subscription and blob streams let slow body
transfers proceed without blocking control operations. JSONL remains the external
provider boundary.

This replaces the earlier Kotlin-owned state and SSH/Phoenix transport design.
Concurrent device control and pairing as the filesystem authorization boundary
remain the decisions in ADR 0002 and ADR 0003. Current execution and delivery
ownership is described in [Session runtime](../SESSION_RUNTIME.md).
