# M5 — NIP-42 auth (protocol module landing report)

**Status:** Partial. This commit ships the **`nmp-nip42` protocol module crate** per the T40 contract in `docs/plan/m8-subscription-lifecycle.md` §3. The M5 milestone exit gates (iOS demo + real-relay validation against an NIP-42-required relay such as nostr.wine) are explicit follow-ups; **do not mark M5 complete on this commit.**

**Milestone reference:** [docs/plan/m5-nip42.md](../../plan/m5-nip42.md).
**Architectural contract:** [docs/plan/m8-subscription-lifecycle.md](../../plan/m8-subscription-lifecycle.md) §3 — T40 wins on the canonical `RelayAuthState` enum, the kind:22242 builder, the AUTH/OK parsers, and the handshake driver. T46 (M8-subs) already shipped the wire pause/flush gate; T43 (M6 signers) already shipped the canonical `Signer` trait.

---

## What this commit ships

### New crate: `crates/nmp-nip42/`

```
crates/nmp-nip42/
├── Cargo.toml            — pure-Rust, depends on nmp-core only (no nmp-nip* deps)
└── src/
    ├── lib.rs            — public surface + module declarations
    ├── state.rs          — canonical `RelayAuthState` + one-way translator to subs placeholder
    ├── frame.rs          — `parse_auth_frame` / `parse_ok_frame`
    ├── builder.rs        — `build_auth_event` (kind:22242 template), validator, wire-frame renderer
    └── flow.rs           — `Nip42Driver` per-relay handshake driver + `run_handshake` helper
```

### Surface details

**`state::RelayAuthState`** matches ADR-0007 §1 exactly:
`NotRequired | ChallengeReceived | Authenticating | Authenticated | Failed`.
`relay_auth_state_to_subs` translates to `nmp_core::subs::trigger::RelayAuthState`
so the lifecycle inbox can fan transitions through `CompileTrigger::
RelayAuthStateChanged` without taking a direct dependency on this crate.

**`frame::parse_auth_frame`** turns `["AUTH", <challenge>]` into an `AuthChallenge { challenge, relay_url }`. Rejects malformed frames (wrong tag, missing/empty challenge, non-string challenge).

**`frame::parse_ok_frame`** turns `["OK", <event_id>, <accepted>, <reason>]` into an `AuthOk`. The driver matches event_id against the in-flight kind:22242; non-AUTH OKs are no-ops by ID.

**`builder::build_auth_event`** produces an `nmp_core::substrate::UnsignedEvent` with kind=22242 and the two mandatory tags (`["relay", url]`, `["challenge", value]`). The signer fills `id`/`pubkey`/`sig`.

**`builder::validate_signed_for`** structural check on the signer's response: catches buggy/malicious signers that mutate kind, drop the challenge tag echo, or return malformed ids. Schnorr verification is the kernel's `verify_and_persist` path, not this crate.

**`flow::Nip42Driver`** the per-relay state machine: holds the in-flight challenge + pending kind:22242 id, exposes `on_auth_frame` / `deliver_signed` / `on_ok_frame` / `reset_on_disconnect`. Each tick returns a `HandshakeOutcome { wire_frames, new_state, failure_reason }` the caller acts on:
- `wire_frames` go to the relay socket verbatim
- `new_state` is fanned into `subs::SubscriptionLifecycle::handle_auth_state_change` to drive `subs::AuthGate`'s pause/flush of held REQs
- `failure_reason` is surfaced via the diagnostic state today; the M10.5 toast-field FFI bridge will eventually carry it to the platform

**`flow::run_handshake`** convenience for callers with a synchronous signer in hand: parses AUTH, builds unsigned, invokes signer, dispatches result — one call.

### Test coverage — 25 tests pass

```text
cargo test -p nmp-nip42
test result: ok. 25 passed; 0 failed
```

- `state::tests` (3) — ADR-0007 wire-key alignment, subs translation totality, default value.
- `frame::tests` (6) — AUTH parser shape + rejection cases; OK parser shape + reason handling + rejection cases.
- `builder::tests` (6) — kind:22242 template construction; validator round-trips for valid events and rejects wrong-kind / missing-echo / malformed-id / empty-sig; wire-frame structural shape.
- `flow::tests` (10) — happy path (challenge → AUTH → OK true → Authenticated); rejected OK surfaces reason and transitions to Failed; signer failure surfaces reason without dispatching wire frame; signer returning structurally-invalid event flagged as failure; unrelated OK is a no-op; reset_on_disconnect clears state and challenge; mid-session re-auth drops back to ChallengeReceived; run_handshake one-call helper; deliver_signed without pending challenge is a no-op.

### Gates

- `cargo build --workspace` — clean.
- `cargo test --workspace --lib` — 100% green (24 nmp-core + 77 publish + 29 subs + 25 nmp-nip42 + others — 157+ total).
- `cargo clippy -p nmp-nip42 --all-targets -- -D warnings` — clean.
- `cargo fmt -p nmp-nip42 -- --check` — clean.
- File sizes (LOC): `src/lib.rs` 59, `src/state.rs` 145, `src/frame.rs` 157, `src/builder.rs` 190, `src/flow.rs` 296, `tests/flow.rs` 258. All under the 300 LOC soft cap.

---

## What this commit deliberately does NOT do

### Out of T40 scope per the M8-subs contract

1. **Wire-frame pause / flush queue.** T46 (M8-subs) already shipped `subs::auth_gate::AuthGate` — owns the per-relay pending REQ buffer, partitions wire frames, drains on `Authenticated`. T40 (this crate) feeds it via the `CompileTrigger::RelayAuthStateChanged` inbox seam.
2. **Kernel-side `handle_text` integration.** The kernel's `kernel/ingest/mod.rs::handle_text` still has `"OK" => {}` and no `"AUTH" =>` arm. Wiring the kernel to call `nmp_nip42::parse_auth_frame` / `Nip42Driver` is the M2-phase-2 wiring task per `docs/plan/m8-subscription-lifecycle.md` §5 — that task replaces the kernel's hand-rolled `req`/`defer_outbound` calls with `SubscriptionLifecycle::drain_tick` + `ConnectionPool::send`. Doing it inside T40 would conflict with that task's scope.
3. **Signer wiring.** T43 (M6) shipped `nmp_signers::Signer::sign(unsigned) -> SignerOp<SignedEvent>`. The protocol module accepts a generic signer closure (`FnMut(&UnsignedEvent) -> Result<SignedEvent, Nip42Error>`); the M6 wiring task adapts the canonical `Signer` trait to that signature at the call site. The publish engine's `publish::traits::Signer::sign_auth` shim is the M7-side path for AUTH-REQUIRED publish retries — different code path, kept separate intentionally.
4. **iOS bridging-header changes.** No FFI surface added — the signer integration is internal Rust, bound by M6's account-manager. The C FFI surface in `NmpCore.h` is unchanged.
5. **Toast field on the FFI snapshot.** The M10.5 toast-field bridge (`docs/design/ffi-hardening.md` §7.2) is the agreed mechanism; this crate exposes `failure_reason` in `HandshakeOutcome` so M10.5 has the data when it lands. Adding a parallel toast-field now would conflict with M10.5's design.

### Required for M5 milestone close-out (not in this commit)

1. **M2-phase-2 wiring task** — kernel ingest calls `nmp_nip42::parse_auth_frame` / `Nip42Driver` / fans `CompileTrigger::RelayAuthStateChanged` into the lifecycle inbox; the lifecycle replaces the kernel's hand-rolled subscription mgmt.
2. **iOS signer integration** — wire an account from `nmp_signers::AccountManager` into the kernel's signer slot so the driver has a real `Signer` to invoke.
3. **Real NIP-42 relay validation** — connect against an actual NIP-42-required relay (e.g. nostr.wine subscriber relay) and verify:
   - Connection completes through AUTH round trip.
   - Subscriptions deliver events after `Authenticated`.
   - `OK false` produces visible diagnostic `Failed` state.
   - Reconnect after sleep/wake re-authenticates without re-issuing logical interests (the subs `InterestRegistry` already guarantees this — the test is empirical confirmation).
4. **Perf measurements** appended to this report. Target: AUTH round-trip < 100 ms with `LocalKeySigner`; bandwidth attribution (kind:22242 wire frames as % of total relay TX).

---

## Coordination notes for downstream tasks

### For the M2-phase-2 wiring task

The integration call sites in `kernel/ingest/mod.rs::handle_text`:

```rust
match kind {
    "AUTH" => {
        if let Some(challenge) = nmp_nip42::parse_auth_frame(array, role.url()) {
            let outcome = self.nip42_drivers
                .entry(role)
                .or_default()
                .on_auth_frame(challenge.clone());
            self.emit_state_to_lifecycle(role, outcome.new_state);
            // Then invoke the bound signer (M6 wiring) and call deliver_signed.
        }
    }
    "OK" => {
        if let Some(ok) = nmp_nip42::parse_ok_frame(array) {
            let outcome = self.nip42_drivers
                .entry(role)
                .or_default()
                .on_ok_frame(&ok);
            outbound.extend(outcome.wire_frames.into_iter().map(|t| OutboundMessage { role, text: t }));
            self.emit_state_to_lifecycle(role, outcome.new_state);
            self.maybe_surface_toast(outcome.failure_reason);
        }
    }
    _ => …
}
```

`emit_state_to_lifecycle` fans the state through `relay_auth_state_to_subs(&state)` into the lifecycle inbox.

### For T39 (NIP-77 negentropy)

Independent. NIP-77 reconciliation runs over an already-authenticated subscription; the protocol layers don't interact except that on relays requiring AUTH, NIP-77 reconciliation messages will queue behind the AUTH round trip via `subs::AuthGate` (correct behavior).

### For the per-relay publish engine in `nmp_core::publish`

The publish engine has its own `Signer::sign_auth` shim for the `AUTH-REQUIRED` publish-retry classification (`publish/state.rs::AckClass::AuthRequired`). Per the advisor review: that's a different code path. The two paths can coexist; the publish engine handles AUTH-REQUIRED responses to its OWN publishes, while `nmp-nip42` handles relay-initiated AUTH challenges. If a future cleanup wants to consolidate, the natural seam is: publish engine uses `nmp_nip42::run_handshake` for its retry, replacing the inline `sign_auth` call. Out of scope here.

---

## Doctrine alignment

- **D0 — kernel never grows app nouns.** This is a protocol-module crate; `nmp-core` gains zero NIP-42 nouns. The integration site is the kernel ingest path, but the protocol logic lives in this crate.
- **D5 — capabilities report, never decide.** The `Signer` reports a signed event; this crate decides the FSM transitions and what to emit on the wire. Pause/flush of held REQs is owned by `subs::AuthGate`.
- **D6 — errors never cross FFI.** `Nip42Error` is internal flow control only; `HandshakeOutcome::failure_reason` is the data point the M10.5 toast-field bridge will read.
- **D8 — reactivity contract.** The driver is stateful but allocation-free per tick (modulo the wire-frame `String` which is unavoidable). One driver per relay; allocations are linear in active relays, not in events.
- **ADR-0007 §1** — `RelayAuthState` enum matches the diagnostics contract exactly with snake-case wire keys (`not_required`, `challenge_received`, `authenticating`, `authenticated`, `failed`).

---

## Why this commit lives in a new crate (not in `nmp-core/src/kernel/auth.rs`)

Master now has the protocol-module crate pattern established (`nmp-nip29`, `nmp-highlighter-core`). The M5 plan (`docs/plan/m5-nip42.md`) calls for `nmp-nip42` explicitly. The justification for keeping NIP-42 inside the kernel (no other protocol modules extracted) no longer applies — NIP-29 has already been carved out. Following the precedent keeps the kernel boundary clean per D0 and makes the M2-phase-2 wiring task's job mechanical.
