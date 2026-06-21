# ADR-0048 — NIP-55 Android signer (Amber) via `ExternalSignerCapability`

- **Status:** Implemented — Stages 1–3 shipped (the `nip55` signer module, the capability wire, the per-op timeout, the `signer_state` projection, and the DM-send path all live in `crates/nmp-signers/src/signers/nip55/` and `nmp-core`). The only open item is Stage 4 (emulator E2E with the Amber APK — see D8).
- **Date:** 2026-06-12
- **Issue:** #1124 (F-13), `priority:p1`, `category:feature`, `area:android`, `area:signer`.
- **Resolves the design half of #1124.** The spec-level decision is already settled in product authority — `docs/product-spec/subsystems.md:134` lists *"External — Android Amber (NIP-55) bridged via `ExternalSignerCapability`"* in the signer catalog. This ADR does **not** relitigate capability-vs-transport; it specifies the concrete shape of that capability bridge.
- **Related:**
  - **ADR-0026** (`RemoteSignerHandle` NIP-44 seal seam) — the actor-facing trait the new signer implements; its `nip44_encrypt` / `nip44_decrypt` methods are reused verbatim for DM seals.
  - **ADR-0031** (`nmp-signer-broker` owns the NIP-46 relay transport) — the canonical *worker-feeds-actor* precedent: a registered hook captures an out-of-`nmp-core` driver, pushes work onto a worker, and re-enters the actor via an `ActorCommand`. NIP-55 reuses this indirection shape exactly, swapping the relay transport for the capability bridge.
  - **ADR-0040** (capability-worker seam) — the serialized off-actor worker + `CapabilityResultReady` re-entry; the timeout/ordering/account-switch invariants here are the foundation this ADR extends from a *synchronous fire-and-return* capability to an *interactive async* one.
  - **ADR-0023 / ADR-0024** (synchronous + async capability protocol history) — the `CapabilityRequest`/`CapabilityEnvelope` socket this bridge rides.
  - **#1098 / V-14** (`bunker_connection_state` typed projection, "KBCS") — the presence/degradation pattern this ADR generalizes into `external_signer_state`.
  - **V-08 / #961** (DM inbox silent failure for bunker accounts) — the receive-side decrypt path; this ADR stages NIP-55 decrypt-for-receive to land *with* that fix, not ahead of it.
  - **V-78** (signer backend must be invisible at the `SignEventForAccount` port) — the binding doctrine this design upholds.

---

## Context

### The gap

NMP has no signer for Android's external-signer ecosystem (Amber / `nostrsigner:` apps). On Android the analogue of NIP-07 (web `window.nostr`) is **NIP-55**: a per-request Android Intent (or, for pre-approved permissions, a background `ContentResolver` query) to a separate signer app that holds the key. The user's key never enters the NMP process — exactly like a NIP-46 bunker, but the transport is an OS IPC round-trip, not a relay round-trip.

The pieces NMP already has, and what each implies:

1. **The actor-facing seam exists and is backend-agnostic.** `crates/nmp-core/src/remote_signer.rs` defines `trait RemoteSignerHandle` with `sign() -> SignerOp<SignedEvent>`, `nip44_encrypt`/`nip44_decrypt`, `pubkey_hex()`, `persistence_payload_json()`, `deliver_rpc_response()`, `disconnect()`. The actor only ever holds `Box<dyn RemoteSignerHandle>` (D0 — `nmp-core` never imports `nmp-signers`). NIP-46 is the sole implementor today (`crates/nmp-signers/src/signers/nip46/handle.rs:24`).

2. **The async-sign machinery already parks remote ops.** A remote sign returns `SignerOp::Pending(rx)` (`crates/nmp-signer-iface/src/op.rs:29`); `sign_active_nonblocking` (`crates/nmp-core/src/actor/commands/identity.rs:698`) hands it back without blocking (D8); the actor parks it as `PendingSign` / `PendingSignReturn` (`crates/nmp-core/src/actor/pending_sign.rs`) and `retain_mut`-polls once per idle tick (`crates/nmp-core/src/actor/mod.rs:2221`). A timed-out op surfaces a D6 toast and a terminal action failure.

3. **The backend-transparent sign port exists.** `ActorCommand::SignEventForAccount { unsigned, signer_pubkey, continuation }` (`crates/nmp-core/src/actor/dispatch.rs:757`) resolves through the *same* nonblocking-sign + park path for both a local nsec and a NIP-46 bunker — "Local-vs-bunker is invisible" (dispatch.rs:769). This is the V-78 port. A NIP-55 signer must slot in here with zero changes to the port.

4. **The capability socket + serialized worker exist.** `CapabilityRequest { namespace, correlation_id, payload_json }` / `CapabilityEnvelope { namespace, correlation_id, result_json }` (`crates/nmp-core/src/substrate/capability.rs:21`) is the JSON-over-FFI carrier; `dispatch_capability` invokes the registered native callback; the ADR-0040 capability-worker (`crates/nmp-core/src/actor/capability_worker.rs`) runs it off-actor and re-enters via `ActorCommand::CapabilityResultReady`. The host registers a callback via `nmp_app_set_capability_callback` (`crates/nmp-ffi/src/capability.rs:30`).

5. **The broker re-entry indirection exists.** `nmp-signer-broker` registers a hook into `nmp-core` (ADR-0031); the hook captures a driver living *outside* `nmp-core`, and after the handshake the driver sends `ActorCommand::AddSigner { source: SignerSource::RemoteHandle(Box<dyn RemoteSignerHandle>) }` (`crates/nmp-core/src/actor/mod.rs:303`) back into the actor. The actor never imports the driver.

6. **The presence projection exists.** `#1098` added `bunker_connection_state` (`BunkerConnectionStateDto`, `crates/nmp-core/src/actor/commands/identity.rs:165`) — a `connected`/`reconnecting`/`failed` slot serialized into `projections` every tick, with pre-computed `is_*` flags so the shell never re-derives state (aim.md §6 / AP1). Hosts render a green dot / amber spinner / red warning.

7. **The signer-app probe table exists and already names `nostrsigner://`.** `signer_apps_table()` (`crates/nmp-core/src/actor/commands/identity.rs:274`) is the Rust-owned list of detectable signer schemes; the `SignerAppDescriptor.signer_kind` field was explicitly designed so *"a future NIP-55 / hardware-signer entry can populate a different kind"* (identity.rs:261). NIP-55 is the entry that field was reserved for.

### Why this is not "just another `RemoteSignerHandle`"

NIP-46 and NIP-55 share the *seam* (both are `RemoteSignerHandle` returning `Pending`) but differ on three axes that the design must address:

- **Transport.** NIP-46 rides a relay (kind:24133 RPC over WebSocket, driven by `nmp-signer-broker`). NIP-55 rides an OS IPC bridge (Android Intent / `ContentResolver`) — i.e. a **capability**, per D7. There is no relay, no ephemeral local key, no kind:24133. The driver is the *host* (Kotlin), not a Rust worker thread.
- **Interactivity / latency.** A NIP-46 bunker can auto-approve in <1s. A NIP-55 Intent round-trip **requires the user to foreground Amber and tap approve** — routinely 5–30s, occasionally longer. The global `PENDING_SIGN_TIMEOUT` (5s, `pending_sign.rs:24`) would abandon almost every interactive sign. This is the single hardest interaction in the design.
- **Account material.** A NIP-46 payload carries an ephemeral local key + remote pubkey + relays (`Nip46Payload`). A NIP-55 account carries **no key material at all** — only the signer's Android package name and the user pubkey. Restart reconstruction is trivial (no handshake), but identity-mismatch on `current_user` is a new failure mode.

---

## Decision

### D1 — Signer placement: `Nip55Signer` implements `RemoteSignerHandle`, returns `SignerOp::Pending`, driven by the capability bridge

A new `crates/nmp-signers/src/signers/nip55/` module defines `Nip55Signer`, a third `RemoteSignerHandle` implementor alongside `Nip46Signer`. It holds:

- `user_pubkey: PublicKey` — returned synchronously by `pubkey_hex()` (cached at construction; no handshake).
- `signer_package: String` — the Amber/`nostrsigner:` Android package name.
- `granted_permissions: Vec<Nip55Permission>` — what the user pre-authorized on first connect (drives the ContentResolver fast-path decision *in the host*, but stored here so it persists).
- a `pending: Arc<Mutex<HashMap<correlation_id, Sender<Result<String, SignerError>>>>>` table — identical in shape to `Nip46Signer::pending` (`crates/nmp-signers/src/signers/nip46/mod.rs:54`).
- an `Arc<dyn ExternalSignerTransport>` — the outbound contract (see D2), the NIP-55 analogue of `Nip46Transport`.

`sign(unsigned)` builds a NIP-55 `sign_event` request, allocates a `correlation_id`, registers a one-shot `Sender` in `pending`, hands the request to the transport, and returns `SignerOp::Pending(rx)` immediately. `nip44_encrypt` / `nip44_decrypt` do the same with `nip44_encrypt` / `nip44_decrypt` methods. The host delivers the raw result back; the signer resolves the matching `Sender` by `correlation_id`. This is structurally identical to how `Nip46Signer` resolves relay RPC responses — only the wire under the transport differs.

**`RemoteSignerHandle::deliver_rpc_response` is renamed `deliver_response`** (one method, both backends) — `nip46/handle.rs:71` and the new nip55 handle both route a decoded, correlation-keyed response into `ingest_response`. This is the *no-compat-alias* hard break: rename in the trait, update both implementors and the actor call site in one PR (the memory note "No compat aliases — ever" binds). The method is already content-agnostic (it takes already-decrypted JSON), so the rename is mechanical.

**Rejected alternative — capability-only, no `Signer` impl.** A "capability that signs" with no `RemoteSignerHandle` would force the actor's sign path to special-case NIP-55 (a second sign code path keyed on signer kind), which is exactly the V-78 violation this design must avoid and a fragmentation D-rule reject. The `SignEventForAccount` port (dispatch.rs:757) is explicitly "backend-transparent"; honoring it *requires* the signer be a `RemoteSignerHandle`. Placement in `nmp-signers` (not `nmp-android-ffi`) keeps it reusable by any Android Nostr app (the NMP-crate test, AGENTS.md) and keeps Android-platform nouns out of the kernel (D0).

### D2 — The capability wire: a new `external_signer` namespace on the existing capability socket; the host owns Intent-vs-ContentResolver

`ExternalSignerCapability` is a `CapabilityModule` (`crates/nmp-core/src/substrate/capability.rs:11`) with `NAMESPACE = "external_signer"`, riding the **existing** `CapabilityRequest`/`CapabilityEnvelope` carrier. A new leaf module (analogous to `nmp-signer-iface`'s `nip46_transport.rs`) defines the typed request/response so both `nmp-core` (which holds the transport trait object) and `nmp-signers` (which builds the requests) can import it without a D0 cycle.

Request (Rust builds it; serialized to `payload_json`):

```rust
struct ExternalSignerRequest {
    correlation_id: String,         // echoed back; matches the pending Sender
    method: ExternalSignerMethod,   // GetPublicKey | SignEvent | Nip44Encrypt | Nip44Decrypt | Nip04Encrypt | Nip04Decrypt
    payload: String,                // NIP-55 payload: unsigned-event JSON for sign_event; plaintext/ciphertext for enc/dec
    current_user: Option<String>,   // hex pubkey; None only for the initial get_public_key
    counterparty: Option<String>,   // hex pubkey for the enc/dec `pubkey` param
    permissions: Vec<Nip55Permission>, // non-empty ONLY on the first get_public_key (permission batch)
    signer_package: Option<String>, // None on first connect (host resolves which app); Some after
}
```

Response (host reports raw; D7 — decides nothing):

```rust
struct ExternalSignerResponse {
    correlation_id: String,
    outcome: ExternalSignerOutcome, // Ok(String) | Rejected | Unavailable(reason) | SignerError(reason)
    signer_package: Option<String>, // populated by the get_public_key reply (NIP-55 returns the package)
}
```

**The Kotlin layer fires what Rust built and reports raw results — it decides nothing** (AGENTS.md "Rust owns all logic"; D7). Specifically:

- **Intent round-trip** (`Intent.ACTION_VIEW`, `Uri.parse("nostrsigner:$payload")`, `registerForActivityResult`) — used whenever the operation may need user approval: the first `get_public_key`, and any method whose permission the user has *not* pre-granted for background use.
- **`ContentResolver` fast-path** (`content://<package>.<METHOD>`, selection args `[payload, counterparty, current_user]`) — used when the method's permission is in `granted_permissions`. No UI; returns `null`/`rejected` if the permission was revoked.

The **rule for which path** is a host-side mechanical consequence of `granted_permissions` (carried in the request), not a policy decision: Kotlin checks "is `method` in the granted set?" and picks the resolver path; otherwise the Intent path. Rust decides *what* to request and *what permissions to ask for*; the host decides only *the OS mechanism*, which is rendering/capability execution. A `ContentResolver` `null` (permission silently unavailable) is reported as `Unavailable`, and the **signer's resolution policy** (in Rust) is to re-issue the same request flagged `force_interactive` so it falls to the Intent path — never the host retrying on its own (D7: native never retries/decides).

**Permission batching** happens on the first `get_public_key`: Rust populates `permissions` with the standing set NMP needs (`sign_event` for the kinds the app publishes, `nip44_decrypt`, `nip44_encrypt`). The user grants them once in Amber; the reply's `signer_package` + the granted set are persisted (D4). This is what enables the ContentResolver fast-path for everything after sign-in.

### D3 — Timeouts: a per-op deadline carried on the parked op, sourced from the signer; `EXTERNAL_SIGN_TIMEOUT` = 90s

> **Implementation note:** the method named `sign_timeout()` below was renamed `op_timeout()` by ADR-0050 §D4; the live method is `RemoteSignerHandle::op_timeout(&self) -> Duration` (`crates/nmp-signers/src/signers/nip55/handle.rs:51`), covered by the `op_timeout_is_90s` test (`crates/nmp-signers/src/signers/nip55/tests.rs:189`).

The 5s `PENDING_SIGN_TIMEOUT` is correct for a relay round-trip and **wrong** for an interactive Intent. The fix is *not* to bump the global constant (that would loosen the bunker timeout and is a single-fact-two-values fragmentation reject) but to make the deadline a **per-op property** sourced from the signer kind:

- `pending_sign.rs` constructors (`PendingSign::new`, `PendingSignReturn::new`/`with_continuation`/`with_target`) gain a `deadline: Instant` parameter instead of computing `Instant::now() + PENDING_SIGN_TIMEOUT` internally. The `timed_out()` check (`pending_sign.rs:122,209`) is unchanged.
- The dispatch arms that park (`dispatch.rs:749,786`) compute the deadline from the **active signer's budget**: a new `RemoteSignerHandle::op_timeout() -> Duration` (default `PENDING_SIGN_TIMEOUT` = 5s; `Nip55Signer` overrides to `EXTERNAL_SIGN_TIMEOUT`). The actor reads it via the handle it already holds — no new protocol noun in `nmp-core` (the constant lives in `nmp-signer-iface`, the kernel only sees a `Duration`).
- `EXTERNAL_SIGN_TIMEOUT = Duration::from_secs(90)`. Rationale: long enough that a user who alt-tabs to Amber, reads the event, and approves does not time out (the NDK research note: *"async and times out — UX needs spinners, not blocking modals"*, `docs/research/ndk/signers.md`); short enough that a forgotten/never-foregrounded request is reclaimed within a session. 60s is too tight for a first-time user; 120s strands the UI spinner too long on abandonment. 90s is the adjudicated value.

The host shows the pending state through the existing **`action_lifecycle`** surface (the dispatched action stays `in_flight` until the parked op resolves) — **no blocking modal**, per the NDK research note. The `external_signer_state` projection (D6) carries an `is_awaiting_approval` flag so a host can render "Waiting for Amber…" inline.

**Why per-op deadline and not a separate park queue:** a third parked-op queue with NIP-55 semantics would duplicate the entire `retain_mut`/poll/timeout/route machinery (`mod.rs:2221`) — a fragmentation reject. The deadline already travels with each op; making it a field, not a hard-coded constant, is the minimal *correct* change and serves every future signer kind (hardware wallets, etc.) for free.

### D4 — Account model: pubkey-only account, `SignerPayload::Nip55`, trivial restart, explicit identity-mismatch handling

A NIP-55 account is **pubkey-only** — no key material ever in the NMP process. `SignerPayload` (`crates/nmp-signers/src/signers/payload.rs:30`) gains a `Nip55` variant:

```rust
Nip55(Nip55Payload)              // mirrors Nip07Payload: no secret to redact, so it may derive Debug
struct Nip55Payload {
    user_pubkey_hex: String,      // the account identity
    signer_package: String,       // which Android signer app holds the key
    granted_permissions: Vec<Nip55Permission>, // restored so the ContentResolver fast-path survives restart
}
```

`SignerBackend` (`payload.rs` neighbor, `traits.rs:22`) gains `Nip55`; `signer_kind()` returns `"nip55"`; the session `Account.signer_kind` (`docs/product-spec/subsystems.md:120`) records it. **Session persistence** uses the existing `RemoteSignerHandle::persistence_payload_json()` path — the broker-restore precedent (`crates/nmp-signer-broker/src/broker/restore.rs`): on restart the host hands the stored payload back, NMP reconstructs `Nip55Signer` directly (no handshake, unlike NIP-46), and re-activates it. This is *cheaper* than bunker restore: there is no relay to reconnect, only a package name to trust.

**`AccountManager` / `switch_active`** treat a NIP-55 account exactly like a bunker account — `active_remote()` returns the handle, `sign_active_nonblocking` (identity.rs:698) takes the remote branch. No change to the account-switch logic.

**Identity mismatch** (Amber returns a `sign_event` whose pubkey ≠ the account's `user_pubkey`, e.g. the user switched the active key inside Amber): the `Nip55Signer` mapper verifies the returned event's pubkey against `self.user_pubkey` (the applesauce `SignerMismatchError` discipline, mirrored by `Nip46Signer`'s mapper) and resolves the op with `SignerError::Mismatch` (`crates/nmp-signer-iface/src/error.rs:30`). The actor surfaces a D6 toast; the account is **not** silently rebound to Amber's current key. A deliberate "the signer now holds a different key" recovery is a re-sign-in, not an automatic swap (D6 — the mismatch becomes state, never a silent correctness violation).

### D5 — Encryption ops: nip44 via the ADR-0026 seam now; NIP-55 decrypt-for-receive staged *with* V-08

**Send path (in scope now).** DM send already routes through `active_signer_for_seal()` (identity.rs:608) → `RemoteSignerForSeal` (ADR-0026) → `RemoteSignerHandle::nip44_encrypt`. Because `Nip55Signer` implements `nip44_encrypt` (D1), the kind:13 seal for a NIP-17 DM is produced by Amber with **zero changes to the DM-send path** — it works for a NIP-55 account the moment the handle exists, exactly as it works for a bunker today.

**Receive path (staged with V-08 / #961).** Inbound DM decryption needs `nip44_decrypt` of the kind:13 seal. `Nip55Signer` will implement `nip44_decrypt` (the method is in the trait), and NIP-55's `ContentResolver` makes a background decrypt **cheaper than a bunker RPC** (no relay round-trip, no UI if the permission is pre-granted) — which is a genuine reason to want it. **But** the receive-side DM-decrypt path itself is the open V-08 residual: bunker accounts *also* silently fail DM-inbox decrypt today (#961, `status:staged`, post-v1). Landing NIP-55 decrypt-for-receive ahead of V-08 would create a second, NIP-55-specific receive path that V-08 would then have to unify — a fragmentation reject. **Adjudication: NIP-55 `nip44_decrypt` ships in the trait impl (so the capability is complete and testable in isolation), but wiring it into the DM-inbox path is explicitly deferred to V-08, where it lands through the same unified receive-decrypt mechanism that fixes bunker decrypt.** `decrypt_zap_event` and NIP-04 are likewise implemented in the capability (the wire supports them) but not wired to any kernel consumer until a consumer exists — no speculative kernel plumbing.

### D6 — Presence/degradation: generalize KBCS into `external_signer_state`

The four states a host must distinguish for a NIP-55 signer — **not installed**, **uninstalled mid-session**, **rejected**, **timeout** — map onto the #1098 KBCS pattern. Rather than a parallel projection, **generalize**: the `bunker_connection_state` DTO (`identity.rs:165`) and the new external-signer presence collapse into one `signer_state` slot+projection keyed by the active account, with a `kind` discriminator (`nip46` | `nip55`) and pre-computed `is_*` flags:

```rust
struct SignerStateDto {
    signer_kind: String,            // "nip46" | "nip55" | "local"
    state: String,                  // "ready" | "awaiting_approval" | "reconnecting" | "unavailable" | "failed"
    reason: Option<String>,
    is_ready: bool,
    is_awaiting_approval: bool,     // NIP-55 Intent round-trip in flight (drives "Waiting for Amber…")
    is_unavailable: bool,           // signer app not installed / uninstalled mid-session
    is_failed: bool,                // permanent error (rejected, mismatch, timeout)
}
```

For NIP-55: `unavailable` is set when the host reports the package is not installed (the `signer_apps_table()` probe at `identity.rs:274` already lets the shell detect installed signers using `canOpenURL`-class platform checks — Android's `PackageManager.queryIntentActivities` is the analogue). **Sign-in is gated on the Kotlin-REPORTED resolvability**: the host probes the package and reports installed/not; Rust gates the "Sign in with Amber" affordance on that reported flag (the host renders; Rust decides whether sign-in is allowed — D7). A mid-session uninstall surfaces as the next op resolving `Unavailable`, flipping the projection to `is_unavailable` so the host prompts re-auth.

Generalizing (vs. a second parallel projection) is the single-canonical-representation D-rule: bunker and external signer are both "remote signer health," and a host's "signer status row" should read one projection regardless of kind. The canonical projection is named **`signer_state`**; the old NIP-46-only `bunker_connection_state` name was removed outright in a hard-break rename (no surviving alias, per the no-compat-alias rule). This was a clean in-place rename — #1098's consumers are all in-repo (iOS `AccountsView`, Android `SignInScreen`) and were updated in the same change.

### D7 — Testability: emulator E2E with the Amber APK; CI on a mocked resolver

- **Emulator E2E (the acceptance oracle).** Install the Amber APK (`github.com/greenart7c3/Amber` releases) on an Android emulator via `adb install`; pre-seed an Amber identity; script approval via `adb shell` UI automation (or Amber's auto-approve-permissions setting for the granted batch). **Acceptance criterion: sign in with Amber → publish a kind:1 through the full kernel pipeline → the published event is signed by the Amber-held key** (verify the event's pubkey == the Amber identity, and the signature validates). A second scenario exercises the ContentResolver fast-path: after the permission batch is granted, a sign with no UI interaction succeeds. This is the F-13 done-gate and lives in the Chirp Android instrumented-test target.
- **CI story.** The emulator+APK E2E is too heavy and too non-deterministic for per-PR CI (it needs an emulator boot + a third-party APK + UI automation). CI runs instead:
  - **Rust unit:** `Nip55Signer` request-building, correlation-id round-trip, mismatch rejection, timeout-budget wiring, payload round-trip — all against a stub `ExternalSignerTransport` (the `Nip46Signer` test pattern, `crates/nmp-signers/src/signers/nip46/handle/tests.rs`). No Android, no emulator.
  - **Kotlin unit:** a **fake signer ContentProvider** (an in-process `ContentProvider` returning canned cursors) and a fake Intent result, asserting the Kotlin bridge fires the right URI/selection-args and reports the raw result envelope unchanged. This is the decide-nothing (D7) contract test.

  The emulator+Amber E2E runs in a dedicated, non-blocking nightly/manual lane — it is the *correctness* oracle, not a merge gate; the unit layers are the merge gates.

### D8 — Staging record (issue #1124)

The work landed as four increments. Stages 1–3 are **shipped**; Stage 4 (emulator E2E) is the only open item.

1. **Stage 1 — capability seam + signer variant (Rust, headless). ✅ Shipped.**
   Delivered: the `external_signer` leaf wire module (`ExternalSignerRequest`/`Response`/`ExternalSignerTransport`), the `Nip55Signer` impl of `RemoteSignerHandle` (`crates/nmp-signers/src/signers/nip55/`), `SignerPayload::Nip55` + `SignerBackend::Nip55`, the per-op `deadline` refactor of `pending_sign.rs` + `RemoteSignerHandle::op_timeout()` + `EXTERNAL_SIGN_TIMEOUT`, the `deliver_rpc_response`→`deliver_response` rename, and the `signer_state` generalization of `bunker_connection_state`. Verified by `cargo test -p nmp-signers` + `-p nmp-core --lib signer_state` against the stub transport and `doctrine_lint_smoke`; no host code.
2. **Stage 2 — Chirp Android sign-in UI. ✅ Shipped.**
   Delivered: the Kotlin `ExternalSignerCapability` bridge (registered via the JNI capability-callback path), the `signer_apps_table()`-driven "Sign in with Amber" affordance gated on reported resolvability, the Intent + ContentResolver dispatch, and the `signer_state` row in `SignInScreen` (green/amber/red, the #1098 Android pattern). A real device/emulator sign-in resolved a `get_public_key` and persisted a pubkey-only account; restart reconstructed it without a prompt.
3. **Stage 3 — DM encryption (send) ops. ✅ Shipped.**
   Delivered: the seal-send path (ADR-0026 seam) produces an Amber-signed kind:13 for a NIP-55 account; the permission batch covers `nip44_encrypt`; the ContentResolver fast-path is used post-grant. A DM sent from a NIP-55 account is gift-wrapped with the seal signed by Amber. (Receive-side decrypt is **out of this feature** — it lands with V-08/#961.)
4. **Stage 4 — emulator E2E. ⏳ Open (only remaining item).**
   To deliver: the `adb`-driven Amber-APK acceptance test (D7) wired into the Chirp Android instrumented-test target and a manual/nightly CI lane. Done-gate: the publish-kind:1-signed-by-Amber scenario passes end-to-end; F-13 closes.

---

## Consequences

**Positive.**
- The V-78 port stays honored: `SignEventForAccount` (dispatch.rs:757) is untouched; NIP-55 is invisible at the sign boundary, identical to a bunker.
- One canonical sign path, one canonical parked-op machinery, one canonical signer-health projection — the per-op `deadline` and the `signer_state` generalization *remove* a hard-coded fork rather than adding one.
- The capability rides the existing `CapabilityRequest` socket; no new FFI primitive (the ADR-0040 worker + `nmp_app_set_capability_callback` already exist). The only genuinely new Rust primitive is the `ExternalSignerTransport` leaf trait — the NIP-55 analogue of the already-precedented `Nip46Transport`.
- Reusable: `Nip55Signer` lives in `nmp-signers`, usable by any Android Nostr app on NMP (the NMP-crate test).

**Negative / accepted trade-offs.**
- The `deliver_rpc_response`→`deliver_response` rename is a (mechanical) breaking change to `RemoteSignerHandle`, touching both implementors + the actor call site in one PR (no-compat-alias rule makes this non-optional).
- `EXTERNAL_SIGN_TIMEOUT = 90s` means a parked NIP-55 op can hold a slot in `pending_sign_returns` for up to 90s. This is a `Vec` of at-most-a-handful of entries scanned once per idle tick — negligible — and the abandonment toast is the existing D6 path.
- NIP-55 `nip44_decrypt` ships implemented-but-not-wired-to-DM-inbox until V-08. This is a deliberate staging (documented here + on #961), not undocumented debt — the trait impl is fully tested in isolation; only the kernel *consumer* waits for the unified receive-decrypt mechanism.

## Alternatives considered (rejected)

1. **Capability-only, no `RemoteSignerHandle` impl** — forces a second sign code path keyed on signer kind; V-78 + fragmentation reject (see D1).
2. **Bump the global `PENDING_SIGN_TIMEOUT` to cover Amber** — loosens the bunker timeout (single-fact-two-values), and 90s is wrong for a relay RPC; rejected for the per-op deadline (D3).
3. **A third NIP-55-specific parked-op queue** — duplicates `mod.rs:2221` machinery; fragmentation reject (D3).
4. **A parallel `external_signer_state` projection beside `bunker_connection_state`** — two representations of "remote signer health"; rejected for generalization (D6).
5. **Drive NIP-55 from a Rust worker thread like `nmp-signer-broker`** — the NIP-55 transport *is* an OS IPC capability owned by the host (Android `Intent`/`ContentResolver` have no Rust-side driver); a Rust worker would have nothing to do but forward to the host. The capability bridge (D2) is the correct, already-precedented shape; this is exactly the spec's "bridged via `ExternalSignerCapability`" (subsystems.md:134).
6. **Wire NIP-55 DM-receive decrypt now** — creates a NIP-55-specific receive path that V-08 must later unify; staged with V-08 instead (D5).
