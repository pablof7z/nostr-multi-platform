---
type: research-record
date: 2026-06-26
session: 1077a92b-e2b0-457d-870e-5e12e4f524cf
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1077a92b-e2b0-457d-870e-5e12e4f524cf.jsonl
source_lines: 3612-3625
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "NIP-46 architectural analysis: evaluated protocol/transport separation criterion; diagnosed conflation in PR #2117; verdict: do not merge, requires decoupled design"
captured_at: 2026-06-26T08:00:37Z
---

NIP-46 architectural analysis: evaluated protocol/transport separation criterion; diagnosed conflation in PR #2117; verdict: do not merge, requires decoupled design

---

Assistant: Codex's verdict is unambiguous — **you're right**, with smoking-gun citations. Here's the synthesis:

## The diagnosis: NIP-46 *is* baking transport + process management

`nmp-signer-broker` conflates pure protocol with connectivity/execution. Its own contract says each handshake "spawns a worker thread" ([lib.rs:31](#)) that opens its **own** relay socket (`handshake_thread.rs:73` → `TungsteniteRelayClient::connect`), creates a **separate `nmp_network::Pool` per session** with native features (`relay_client.rs:153`), spawns **its own dispatcher thread** (`relay_client.rs:222`), and owns a **relay intake queue** (`intake.rs:11`). That's why it can't go to wasm and why #2117 had to spawn a thread-per-sign — the entanglement propagates.

## The good news
The *pure* NIP-46 already exists in the right place: `Nip46Signer` (`nmp-signers/.../nip46/mod.rs`) is **already transport-injected** via a `Nip46Transport` trait, parks RPCs by id, and resolves inbound responses. And both platforms already expose the transport seam NIP-46 should ride:
- **native** — actor relay lane sends frames (`relay_mgmt.rs:339`), inbound text → interceptors (`relay_events.rs:185`)
- **browser** — `BrowserRelayDriver` + `RelayPool::fan_out_outbound` (`spawn.rs:40`), inbound → `run_relay_text_interceptors`

## The target (matches your instinct exactly)
A transport-agnostic **`nmp-nip46` protocol core** shaped as an event reducer — `start_bunker`/`on_relay_text`/`on_relay_connected`/`send_rpc` that **emit** "send this frame" / "subscribe" effects and **park** pending ids. It owns parsing, NIP-44, handshake state, id correlation. It owns **no socket, no Pool, no threads, no hidden clocks**. Native drives it from the actor relay lane; browser from the pump + `BrowserRelayDriver`. **NIP-46 spawns nothing and opens nothing** — exactly "NIP-46 does NIP-46, transport is someone else's job."

