# Opus Direction Review #4 — The FFI Contract Surface (2026-05-20)

Scope: the entire FFI contract, not just errors. New insight this round: the
contract is **asymmetric**, and that asymmetry is its real liability.

## 1. Shape of the contract today

Two halves, typed at two different layers:

- **Commands (Swift→NMP)** are many individual `#[no_mangle] extern "C"`
  symbols — `nmp_app_signin_nsec`, `nmp_app_publish_note`,
  `nmp_app_publish_signed_event_to`, etc. (`ffi/identity.rs:13–402`). These
  are typed *at the symbol level*: calling a command that does not exist is a
  **link-time** failure, not a runtime decode failure. That is genuinely good
  — and answers question 3's "what if Swift calls a missing command": it
  cannot, the binary won't link.
- **Updates (NMP→Swift)** are one channel, JSON, discriminated by the
  `UpdateEnvelope` tag (`update_envelope.rs:89–100`). Typed *at the payload
  level*.

So the surface is half C-ABI-typed, half stringly-typed — and the stringly
half is larger than it looks.

## 2. The stringly-typed soft spots

The *payloads inside* commands are JSON strings the FFI layer re-parses:
`profile_json`, `relays_json` (`identity.rs:41–78`), `event_json`,
`unsigned_json` (`identity.rs:135–316`). Malformed JSON no longer silently
drops (the `e895c09` fix routes a `ShowToast`), but the failure is *typed as
prose*: `"Failed to decode profile JSON"`.

The second soft channel is `dispatch_capability` (`ffi/mod.rs:499–510`) — a
full Swift↔Rust JSON round-trip carrying a `correlation_id`. Its
`unwrap_or_else` fallbacks (`"{}"`, `{"status":"error","os_status":-50}`)
have **no schema version at all** — a second silent versioning gap beyond
the snapshot one below.

## 3. The missing seam: command/update correlation

`dispatch_capability` carries a `correlation_id` — proving the project knows
the pattern. But the publish/identity command surface does **not** use it.
Swift calls `nmp_app_publish_note`; what comes back is a *future snapshot*
that may or may not contain the note, plus possibly a `ShowToast` with no
link to the originating call. iOS cannot answer "did *my* publish fail?" —
only "a publish-ish error appeared." The correlation pattern exists; it just
isn't wired to the surface that needs it most.

## 4. Versioning: protects the snapshot, not the deltas

`SNAPSHOT_SCHEMA_VERSION = 1` (`update_envelope.rs:58`) is carried in every
snapshot (`actor/tick.rs:187`) and a host mismatch fails loudly — correct.
But it guards **only** the snapshot shape. `UpdateEnvelope::Update(KernelUpdate)`
carries no version, and `KernelUpdate` uses default externally-tagged serde.
Adding or renaming a `KernelUpdate` variant is therefore **silently breaking**
on the host until its strict decode happens to fire. That is the loud-vs-silent
divider worth naming: snapshot field changes = loud; discrete-update variant
changes = silent.

## 5. Minimal typed-error contract (non-breaking)

The precedent already exists. `RelayStatus` carries `auth`,
`nip77_negentropy`, `last_close_reason` as **fixed diagnostic string keys**
(`status.rs:85–103`), not free text. Extend that:

- Add `error_category: Option<String>` next to `last_error` on `RelayStatus`
  (`status.rs:99–104`) with a closed key set:
  `auth_required | transient | permanent | malformed_event | policy_denied`.
- Add the same field next to `last_error_toast` for command-originated
  errors.
- Optional fields are non-breaking — Swift `Codable` tolerates absence, so
  `schema_version` stays `1` until a real break.
- Producers are the **existing** `set_last_error_toast` callsites
  (`commands/publish.rs`, `commands/wallet.rs`, `commands/relays.rs`,
  `actor/dispatch.rs:318`). They already know the category at the callsite —
  the malformed-JSON path is `malformed_event`, the NIP-42 path is
  `auth_required`, a timed-out remote sign is `transient`.

This makes iOS able to branch on error *class* without parsing English, and
costs one optional field — no schema bump, no breaking change.
