---
type: research-record
date: 2026-06-26
session: 1077a92b-e2b0-457d-870e-5e12e4f524cf
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1077a92b-e2b0-457d-870e-5e12e4f524cf.jsonl
source_lines: 5880-5999
agent_attribution: a92122d7d902afec4
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Root-cause analysis and option evaluation for NIP-46 reconnect-ordering race condition; verdict: worker-resident preamble approach recommended"
captured_at: 2026-06-26T12:59:23Z
---

Root-cause analysis and option evaluation for NIP-46 reconnect-ordering race condition; verdict: worker-resident preamble approach recommended

---

User: [Agent task result: Agent "Design NIP-46 reconnect-ordering fix" finished]
I have traced the complete path. Let me write the fix design.

---

# Fix design — PR-A #2142 BLOCKER: REQ-before-EVENT flush-order race on NIP-46 reconnect

## 1. Root-cause confirmation — the flush-order race is real and unwinnable from the actor side

The REQ-before-EVENT invariant is violated because two independent threads write to the same socket and the **relay worker thread flushes its `pending` buffer the instant it connects, before the actor thread has even observed the connect**, let alone emitted a replay REQ.

Concrete sequence, with cites:

1. While the bunker socket is down, the worker sits in `wait_before_reconnect` and **accumulates any frame sent to it into `pending`**: `crates/nmp-network/src/relay_worker/mod.rs:474-475` (`Ok(RelayCommand::Send(text)) => pending.push_back(text)`). A sign EVENT (kind:24133) produced by `ActorLaneTransport::send_rpc` → `enqueue_outbound` (`crates/nmp-nip46-runtime/src/transport.rs:81-85`) → `EnqueueOutbound` dispatch (`crates/nmp-core/src/actor/dispatch/mod.rs:206-208`) → `send_outbound` → `Pool::send` (`crates/nmp-network/src/pool/mod.rs:220-232`) lands in exactly this buffer. `Pool::send`'s own doc says so: "the frame is queued in `relay_worker`'s `pending` buffer until the socket opens" (`crates/nmp-network/src/pool/mod.rs:215-219`).

2. The worker reconnects, emits `Connected` **first** (`crates/nmp-network/src/relay_worker/mod.rs:178-187`), then immediately enters `run_connected_relay`, whose very first loop iteration drains the control channel into `pending` and **flushes `pending` FIFO to the socket** (`mod.rs:282-309`; flush is front-to-back in `socket_io.rs:56` `pending.pop_front()`). The queued sign EVENT goes out on the wire here.

3. Only *after* the worker emits `Connected` does the actor thread receive it as `PoolEvent::Opened` and run the connected hooks (`crates/nmp-core/src/actor/dispatch/relay_events.rs:121-126`). The NIP-46 hook then posts the replay REQ via `EnqueueOutbound` (`crates/nmp-nip46-runtime/src/connected_hook.rs:70-74`), which takes a **second** actor-inbox hop before reaching `Pool::send` → the worker's control channel.

So the REQ reaches the worker strictly after the worker has already flushed the EVENT. The bunker's encrypted response to that EVENT arrives on a subscription that is not yet live → `SessionState::on_relay_text` only matches frames for the active sub id (`crates/nmp-nip46/src/reducer.rs:186`), the response is dropped, and the sign strands.

Key structural facts that bound the solution space:
- The worker has **no subscription knowledge** — its only outbound state is `pending: VecDeque<String>` (`mod.rs:159`). There is **no worker-level persistent-sub registry today**.
- The kernel's persistent-sub registry (`kernel.wire.persistent`, `kernel_relay_config.rs:77-97`) and the reconnect replay (`relay_events.rs:127-132`, `kernel.replay_on_reconnect`) both deliver REQs through `Pool::send` **after** the actor processes `Opened` — i.e. they share the *same* race. NWC (`crates/nmp-nip47/src/runtime/commands.rs:122` + REQ at `:134-141`) has the same latent ordering exposure; it is masked only because NWC gates `pay_invoice` on `status == "ready"`.
- Therefore **no actor-side ordering trick (EnqueueOutbound FIFO, direct `Pool::send` in the `Opened` arm, hook priority) can win** — the worker flushes before the actor acts. The fix must make the worker itself send the REQ before it drains `pending`.

The hook's doc comment (`connected_hook.rs:8-19`, `56-69`) asserts the EnqueueOutbound FIFO "guarantees REQ-before-EVENT." That claim is **false**: it reasons only about the actor inbox and ignores the worker's autonomous `pending` flush. This is the root cause.

## 2. Recommended fix — Option (a): a worker-resident reconnect preamble

Give each relay worker a small, generic "on-(re)connect preamble" of frames it flushes **before** draining `pending` on every connect. Register the NIP-46 REQ frame as the Signer lane's preamble once. This makes REQ-before-EVENT a **structural property of the socket**, independent of any actor/worker timing, and cannot regress.

This is viable because the NIP-46 replay REQ is **static across reconnects**: `SessionState::on_relay_connected` builds it from the stable `sub_id` + local pubkey with `since = 0` (`crates/nmp-nip46/src/reducer.rs:195-203`). One registration is correct for the session's lifetime.

### Why (b) and (c) are worse

- **Option (b) — gate EVENT emission in the runtime until the sub is confirmed live.** The EVENT that strands is buffered **in the worker**, not the runtime, so gating only helps if `send_rpc` *never* hands an EVENT to the worker while the socket is mid-reconnect. That requires the runtime to track connection state precisely (connected/disconnected). The runtime today gets a connect signal (the connected hook) but **no disconnect signal** — there is no Closed/Failed callback into `Nip46RuntimeHandle`. Adding reliable connection-state tracking, an EVENT hold-back buffer, and an in-order release on every reconnect is more code in the protocol crate, and a single missed disconnect transition silently reintroduces the bug. It is a correctness-by-vigilance design; (a) is correctness-by-construction.
- **Option (c) — combination.** Unnecessary once (a) makes the socket ordering structural. Keeping (b)'s gating on top adds fragility for no additional guarantee.

Option (a) also fixes the same latent race for any future persistent-sub protocol and (optionally, later) for NWC, with no protocol nouns leaking into `nmp-network` (the preamble is "frames to flush first on connect" — substrate-clean, consistent with the D0 boundary noted in `role.rs:1-13`).

### File-by-file changes

**A. `nmp-network` — worker preamble (the structural guarantee)**

1. `crates/nmp-network/src/relay_worker/mod.rs:75-84` — add a variant:
   `RelayCommand::SetReconnectPreamble(Vec<String>)` (last-write-wins, mirrors the `SetBackoffHint` one-shot pattern at `:78-83`).
2. `crates/nmp-network/src/relay_worker/mod.rs:159` — add worker-scoped state next to `pending`: `let mut preamble: Vec<String> = Vec::new();` (lives in `run_relay_worker` so it survives across reconnect iterations, exactly like `pending`/`backoff`).
3. `crates/nmp-network/src/relay_worker/io_ready.rs:71-86` — thread a `preamble: &mut Vec<String>` param into `drain_pending` and handle the new variant: `Ok(RelayCommand::SetReconnectPreamble(frames)) => *preamble = frames`. Also handle it in `wait_before_reconnect` (`mod.rs:474-483`) so a preamble set during the reconnect wait is stored, not dropped.
4. `crates/nmp-network/src/relay_worker/mod.rs:188-193` — **right after** `RelayEvent::Connected` is sent and before `run_connected_relay` runs its first flush, inject the preamble at the **front** of `pending`, in order:
   `for f in preamble.iter().rev() { pending.push_front(f.clone()); }`
   This places `[REQ…, <EVENTs already queued during the wait>…]` on the wire in the correct order. New EVENTs arriving after connect are appended to the back by `drain_pending` (`io_ready.rs:78`), so they too follow the REQ. (Pass `&mut preamble` into `run_connected_relay` so its in-loop `drain_pending` calls can keep updating it.)

**B. `nmp-network` — pool surface**

5. `crates/nmp-network/src/pool/mod.rs` — add `pub fn set_reconnect_preamble(&self, h: RelayHandle, frames: Vec<String>) -> bool`, copying the `set_backoff_hint` shape (`pool/mod.rs:244-249`): resolve `command_tx_for(h)` and send `RelayCommand::SetReconnectPreamble(frames)`. Add the matching `set_reconnect_preamble_for` on `PoolInner` next to `set_backoff_hint_for`.

**C. `nmp-core` — registration seam (handle exists only on the actor thread)**

6. `crates/nmp-core/src/actor/actor_command.rs:97` — add `ActorCommand::SetReconnectPreamble { role: RelayRole, relay_url: String, frames: Vec<String> }` (sibling of `EnqueueOutbound` at `:97`).
7. `crates/nmp-core/src/actor/inbox/typed_sender.rs:125-132` — add `CommandSender::set_reconnect_preamble(role, relay_url, frames)` (sibling of `enqueue_outbound`).
8. `crates/nmp-core/src/actor/dispatch/mod.rs` — add a dispatch arm that resolves the handle from `ctx.relay_runtime.relay_controls.get(&CanonicalRelayUrl::parse_or_raw(&relay_url))` and calls `ctx.pool.set_reconnect_preamble(control.handle, frames)`. `ActorContext` already exposes both `relay_runtime` and `pool` (`dispatch/mod.rs:94-101`), and the `Frame` arm already does exactly this handle-resolution + `pool.set_backoff_hint` pattern (`relay_events.rs:218-225`), so this is a faithful copy.

**D. `nmp-nip46-runtime` — register the preamble, stop racing on reconnect**

9. `crates/nmp-nip46-runtime/src/connected_hook.rs:61-84` — change the `Effect::Subscribe` handling. Instead of `enqueue_outbound`-ing the REQ on every reconnect (which loses the race), call `command_sender.set_reconnect_preamble(RelayRole::Signer, eff_url, vec![frame])` so the **worker** owns the REQ-first guarantee. On the first `Opened` (`is_reconnect == false`) the initial REQ is already on the wire via the session-init Subscribe effect (`interceptor.rs:118-131` → `send_outbound`), and registering the preamble here arms every subsequent reconnect. (A sign EVENT cannot be queued before the first successful connect+handshake because `ActorLaneTransport` is not built until `Effect::SignerReady`, `interceptor.rs:168-213` — so the first-connect window is safe by construction.)
   - `Effect::SendFrame` on reconnect (e.g. a `connect` resend) is still fine via `enqueue_outbound`: it is itself a frame the bunker handles independent of the inbound sub, and it will be ordered after the preamble REQ anyway once it reaches the worker.

This keeps `EnqueueOutbound` as the steady-state EVENT seam unchanged; the only behavioral change is that the *reconnect REQ replay* moves from a racing actor-inbox post to a worker-resident preamble.

## 3. Deadline-arm-after-REQ-live fix

Today `Nip46Runtime::on_relay_connected` arms the 60 s step deadline synchronously at connect time (`crates/nmp-nip46-runtime/src/runtime.rs:83-88`), before the REQ is live — the SHOULD-FIX. The precise "subscription is installed and the relay has caught us up" signal already on the wire is **EOSE for our sub id** (the mock emits it at `mock_bunker_relay.rs:197-200`; real relays send it after replaying stored events).

Change:
- `crates/nmp-nip46-runtime/src/runtime.rs:79-89` — remove the unconditional `self.state.arm_deadline(now_secs)` from `on_relay_connected`. The hook then only registers the preamble (and emits progress).
- `crates/nmp-nip46/src/reducer.rs:170-191` — extend `SessionState::on_relay_text` to recognize `["EOSE", <our sub_id>]` (it currently early-returns on any non-`EVENT` frame at `:179`). On EOSE-for-our-sub, call `self.arm_deadline(now)`. This arms the step budget from the moment the subscription is confirmed live, exactly as requested, and naturally re-arms on every reconnect's EOSE.

Owner-decision / safety net: a relay that never sends EOSE would never arm the step deadline via this path. The handshake phases (`BunkerWaitConnectAck`/`WaitGpk`) still carry their own deadline armed at `start_bunker`/`start_nostrconnect`, so a stuck handshake is still bounded; but if the team wants a hard floor for the reconnect case, keep a coarse `arm_deadline` at connect as an upper bound and let EOSE tighten it. I recommend EOSE-only for correctness and flagging the no-EOSE relay as the one owner-decision here.

## 4. Deterministic green-gate test

Goal: prove that an EVENT queued *during* a reconnect is never flushed before the replayed REQ on the new socket. Build it on the existing `MockBunkerRelay` harness (`crates/nmp-testing/tests/common/mock_bunker_relay.rs`) plus a forced reconnect.

Required harness additions (test-only, no production change):
1. Record **per-connection frame arrival order**, not just methods. Add an `Arc<Mutex<Vec<String>>>` `frame_log` that `run_connection` pushes `"REQ"` (`mock_bunker_relay.rs:181`) and `"EVENT"` (`:202`) into in arrival order, scoped/markable per connection (e.g. push `"OPEN"` on each new `run_connection` entry at `:141` so the test can isolate the post-reconnect connection's frames).
2. A way to force a reconnect: add a method that flips a per-connection "drop now" flag so the current worker returns from `run_connection` (closing the socket) while leaving the acceptor alive to take the reconnect. The worker's reconnect path (`relay_worker/mod.rs:204-221`) will redial.

Test body (`crates/nmp-testing/tests/nip46_req_before_event_on_reconnect.rs`):
1. Spawn `MockBunkerRelay`, drive a NIP-46 bunker session to `SignerReady` (reuse the setup in `crates/nmp-testing/tests/nip46_bunker_signing.rs`). Assert the steady-state signer is live.
2. Force the mock to drop the connection. **Before** the worker redials (while the socket is down), trigger a sign so a kind:24133 EVENT is produced and lands in the worker's `pending` (`relay_worker/mod.rs:474-475`). Deterministic ordering is achieved by holding the mock's acceptor from accepting until after the sign is enqueued (e.g. a gate the test releases), removing wall-clock flakiness.
3. Release the acceptor; the worker reconnects and flushes.
4. Assert on the post-reconnect connection's `frame_log`: the index of the first `"REQ"` is **less than** the index of the first `"EVENT"`. With the fix (worker preamble injected at front of `pending`, `relay_worker/mod.rs:188-193`) this holds; without it, the EVENT precedes the REQ and the test fails.
5. Stronger end-to-end assertion: the forced-reconnect sign actually **resolves** (the bunker's response is delivered because the sub was live) rather than stranding/timing out.

This frame-order assertion is the green-gate: it fails on `master`/current PR-A and passes only with the worker-preamble fix. Pair it with a pure `nmp-network` unit test in `crates/nmp-network/src/relay_worker/tests.rs` that sets a preamble, queues a `Send` during the reconnect wait, and asserts the preamble frame is written to the socket before the queued frame — this pins the structural guarantee without the full NIP-46 stack.

## 5. Containment

Not contained to `nmp-nip46-runtime`. The structural fix necessarily touches `nmp-network` (the worker is the only place that can order frames before `pending`) and `nmp-core` (the worker handle lives behind the pool and is resolvable only on the actor thread). The footprint is small and additive:
- `nmp-network`: one new `RelayCommand` variant, one worker field, a front-injection on connect, one `Pool` method + `PoolInner` helper. ~30-40 LOC, all mirroring the existing `SetBackoffHint` plumbing.
- `nmp-core`: one `ActorCommand` variant, one `CommandSender` method, one dispatch arm. ~25 LOC, all mirroring `EnqueueOutbound` + the `set_backoff_hint` dispatch in the `Frame` arm.
- `nmp-nip46-runtime`: swap the reconnect REQ from `enqueue_outbound` to `set_reconnect_preamble` (`connected_hook.rs`), drop the eager `arm_deadline` (`runtime.rs`).
- `nmp-nip46`: EOSE recognition + arm in the reducer (`reducer.rs`).

No protocol nouns enter `nmp-network`; the preamble is a generic frame list.

## 6. Risks and owner-decisions

- **Double REQ on first connect.** If the preamble is ever applied on a connect where the init REQ also goes out, the relay receives two identical REQs for the same sub id. This is harmless (a relay replaces the subscription on a repeat REQ), but the design avoids it: the preamble is registered *in the connected hook on Opened*, so it is only present for *subsequent* reconnects, never the first connect. Verify no path applies it twice.
- **Preamble staleness.** Safe here because the NIP-46 REQ is static (`reducer.rs:201`, `since = 0`). If a future protocol needs a per-reconnect-regenerated REQ (e.g. a moving `since` watermark), a static worker preamble would replay a stale filter. Owner-decision: keep the preamble NIP-46-scoped for now; do **not** generalize it to NWC/content reconnect replay in this PR without re-checking each protocol's REQ for reconnect-time mutability.
- **No-EOSE relays and the deadline.** Per §3, arming the step deadline only on EOSE means a relay that omits EOSE won't arm via this path. Owner-decision: EOSE-only (tightest, recommended) vs. keep a connect-time coarse floor.
- **NWC parity.** NWC shares the same latent reconnect race (`commands.rs:122-141`); it is currently masked by the `status == "ready"` gate on `pay_invoice`. Out of scope for this blocker, but worth a tracking issue — the new `set_reconnect_preamble` seam is the natural fix there too.

### Critical Files for Implementation
- /Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-network/src/relay_worker/mod.rs
- /Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-network/src/relay_worker/io_ready.rs
- /Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-network/src/pool/mod.rs
- /Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-nip46-runtime/src/connected_hook.rs
- /Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/actor/dispatch/mod.rs
