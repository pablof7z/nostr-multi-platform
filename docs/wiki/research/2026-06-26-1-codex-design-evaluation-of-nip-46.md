---
type: research-record
date: 2026-06-26
session: 1077a92b-e2b0-457d-870e-5e12e4f524cf
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1077a92b-e2b0-457d-870e-5e12e4f524cf.jsonl
source_lines: 3587-3626
agent_attribution: codex
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Codex design evaluation of NIP-46 architecture: assesses whether it conflates protocol with transport/process management; recommends decoupling via transport-agnostic protocol core; verdict do-not-merge on PR #2117"
captured_at: 2026-06-26T09:01:59Z
---

Codex design evaluation of NIP-46 architecture: assesses whether it conflates protocol with transport/process management; recommends decoupling via transport-agnostic protocol core; verdict do-not-merge on PR #2117

---

Let me run the design consult you asked for — pointing `codex exec` at the actual `nmp-signer-broker` / `Nip46Signer` code to judge whether NIP-46 is conflating protocol with transport + process management, and what the decoupled design should be.

User: [Tool result]: File created successfully at: /private/tmp/claude-501/-Users-pablofernandez-Work-nostr-multi-platform/1077a92b-e2b0-457d-870e-5e12e4f524cf/scratchpad/codex-nip46-design.md (file state is current in your context — no need to Read it back)

User: [Tool result]: Command running in background with ID: bnlkocbdw. Output is being written to: /private/tmp/claude-501/-Users-pablofernandez-Work-nostr-multi-platform/1077a92b-e2b0-457d-870e-5e12e4f524cf/tasks/bnlkocbdw.output. You will be notified when it completes. To check interim output, use Read on that file path.

Assistant: The design consult is running. Let me record the hold on #2117 while we resolve the architecture.

User: [Tool result]: Updated task #4 description

Assistant: Holding #2117 — both because codex found 3 blockers in it and because your architectural question (does NIP-46 bake its own transport + thread-spawning?) needs answering first. The design consult is running now; it'll judge `nmp-signer-broker` against the repo's own thesis (protocol crates own behavior, not transport/execution) and propose the decoupled design — a transport-agnostic NIP-46 core that parks requests and resolves them on inbound relay frames (event-driven, no threads), driven by whichever relay transport each platform already has. I'll bring you its recommendation + my synthesis when it completes, then we decide the path. Standing by.

User: [Tool result]: 1	chpwd:2: command not found: _iterm2_set_profile
2	The core API should look like an event reducer:
3	
4	- `start_bunker(uri, now, rng)` -> emits `Subscribe` and `SendFrame { relay_url, text }`.
5	- `start_nostrconnect(relay_url, perms, now, rng)` -> emits URI plus relay subscription/send effects.
6	- `on_relay_text(relay_url, text, now)` -> advances state and emits `SendFrame`, `Progress`, `SignerReady`, `DeliverResponse`, or `Error`.
7	- `on_relay_connected(relay_url, is_reconnect)` -> emits required subscription frames.
8	- `send_rpc(method, params)` or equivalent -> emits `SendFrame` and parks a pending id.
9	
10	Native would drive that from the existing actor relay lane. Browser would drive the same core from the existing worker pump and `BrowserRelayDriver`. The only platform-owned code is transport execution: native pool threads remain in `nmp-network`; browser WebSocket callbacks remain in `nmp-browser-runtime`. NIP-46 itself spawns nothing and opens nothing.
11	
12	This is inc

Assistant: Codex's verdict is unambiguous — **you're right**, with smoking-gun citations. Here's the synthesis:

## The diagnosis: NIP-46 *is* baking transport + process management

`nmp-signer-broker` conflates pure protocol with connectivity/execution. Its own contract says each handshake "spawns a worker thread" ([lib.rs:31](#)) that opens its **own** relay socket (`handshake_thread.rs:73` → `TungsteniteRelayClient::connect`), creates a **separate `nmp_network::Pool` per session** with native features (`relay_client.rs:153`), spawns **its own dispatcher thread** (`relay_client.rs:222`), and owns a **relay intake queue** (`intake.rs:11`). That's why it can't go to wasm and why #2117 had to spawn a thread-per-sign — the entanglement propagates.

## The good news
The *pure* NIP-46 already exists in the right place: `Nip46Signer` (`nmp-signers/.../nip46/mod.rs`) is **already transport-injected** via a `Nip46Transport` trait, parks RPCs by id, and resolves inbound responses. And both platforms already expose the transport seam NIP-46 should ride:
- **native** — actor relay lane sends frames (`relay_mgmt.rs:339`), inbound text → interceptors (`relay_events.rs:185`)
- **browser** — `BrowserRelayDriver` + `RelayPool::fan_out_outbound` (`spawn.rs:40`), inbound → `run_relay_text_interceptors`

## The target (matches your instinct exactly)
A transport-agnostic **`nmp-nip46` protocol core** shaped as an event reducer — `start_bunker`/`on_relay_text`/`on_relay_connected`/`send_rpc` that **emit** "send this frame" / "subscribe" effects and **park** pending ids. It owns parsing, NIP-44, handshake state, id correlation. It owns **no socket, no Pool, no threads, no hidden clocks**. Native drives it from the actor relay lane; browser from the pump + `BrowserRelayDriver`. **NIP-46 spawns nothing and opens nothing** — exactly "NIP-46 does NIP-46, transport is someone else's job."

Codex's verdict on #2117: **do not merge** — thread-per-sign + native-only broker entrenches the wrong split. I'm closing it.
