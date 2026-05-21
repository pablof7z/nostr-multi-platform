//! Actor-local identity runtime + sign-in / switch / remove handlers.
//!
//! D4: the actor thread is the single writer of identity facts. The
//! authoritative store is the `HashMap<IdentityId, Keys>` here; the kernel's
//! `accounts` projection is pushed via `Kernel::set_accounts` after every
//! mutation, then emitted.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_signer_iface::SignerOp;
use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::{EventBuilder, Keys, Kind, PublicKey, SecretKey, Tag, Timestamp};
use serde::{Deserialize, Serialize};

use crate::actor::{canonical_relay_role, has_role};
use crate::kernel::{AccountSummary, Kernel, RelayEditRow};
use crate::relay::{canonical_relay_url, OutboundMessage};
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::{SignedEvent, UnsignedEvent};

/// NIP-46 bunker handshake progress — the app noun projected onto the snapshot
/// under `projections["bunker_handshake"]`.
///
/// D0: NIP-46 remote signing is an app noun, not a kernel primitive. This type
/// lives in the identity command runtime (the actor layer), NOT in
/// `KernelSnapshot`. The actor writes it to a [`BunkerHandshakeSlot`]; a
/// built-in snapshot projection serializes it into the snapshot's
/// `projections` map every tick (D0 — the kernel emits, never names an app
/// noun).
///
/// `Deserialize` is retained so Swift codegen / round-trip tests can decode it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct BunkerHandshakeDto {
    /// `"connecting"` | `"awaiting_pubkey"` | `"ready"` | `"failed"` | `"idle"`
    /// (the wire never carries `"idle"`; the actor maps it to `None`).
    pub(crate) stage: String,
    /// Optional human-readable status (e.g. relay URL, error reason).
    pub(crate) message: Option<String>,
}

/// Shared bunker-handshake slot — the output side of the bunker projection.
///
/// One `Arc` clone lives on the actor's [`IdentityRuntime`] (the sole writer,
/// D4); another is captured by the built-in `"bunker_handshake"`
/// snapshot-projection closure registered on `NmpApp`. The projection reads
/// this slot on every snapshot tick and serializes its contents into
/// `KernelSnapshot::projections`.
///
/// `None` (the default) means no handshake is in flight — the projection then
/// contributes JSON `null` under the `"bunker_handshake"` key, preserving the
/// "key present, value null when idle" semantic host sign-in flows
/// decode (an explicit `"idle"` stage from the broker maps to `None`).
pub(crate) type BunkerHandshakeSlot = Arc<Mutex<Option<BunkerHandshakeDto>>>;

/// Construct a fresh, empty [`BunkerHandshakeSlot`].
pub(crate) fn new_bunker_handshake_slot() -> BunkerHandshakeSlot {
    Arc::new(Mutex::new(None))
}

/// `SignerOp::wait` timeout for remote-signer signs.
///
/// This blocks the actor thread — relay ingest, subscription management, and
/// UI emits all stall for its full duration. The previous 45s value froze the
/// whole actor for up to 45 seconds on every NIP-46 sign; 5s bounds that worst
/// case while a non-blocking `SignerOp::poll` path is the documented follow-up.
///
/// Trade-off: 5s is too short to cover an interactive user-approval tap on the
/// bunker device. If the remote does not turn around within 5s the sign fails
/// with `SignerError::Timeout`, which `sign_active` formats into a string and
/// the publish callsites (`publish.rs`) surface as `last_error_toast` per D6 —
/// the user sees a toast and re-issues the action rather than the actor
/// wedging. A fast (already-approved / auto-approving) bunker comfortably
/// completes inside 5s.
const REMOTE_SIGN_TIMEOUT: Duration = Duration::from_secs(5);

/// IdentityId is the hex pubkey (matches NDK / applesauce / `AccountManager`).
pub(crate) type IdentityId = String;

/// Actor-local multi-account state. Insertion-ordered for deterministic UI.
///
/// Local-key accounts (nsec / generated) live in `keys`; remote-signer
/// accounts (NIP-46 bunker today, NIP-07 / hardware later) live in
/// `remote_signers`. Both share the same `order` list so the UI projection
/// stays deterministic. If the same pubkey lands in BOTH maps, the remote
/// signer wins (`active_signer_kind` + `sign_active` consult it first) — the
/// user explicitly added a remote handle, so route through it.
pub(crate) struct IdentityRuntime {
    keys: HashMap<IdentityId, Keys>,
    remote_signers: HashMap<IdentityId, Box<dyn RemoteSignerHandle>>,
    order: Vec<IdentityId>,
    active: Option<IdentityId>,
    /// Shared output slot for the bunker-handshake projection. The actor (this
    /// runtime) is the sole writer (D4); the built-in `"bunker_handshake"`
    /// snapshot projection reads it. D0: NIP-46 remote signing is an app noun,
    /// so handshake state is NOT a typed `KernelSnapshot` field.
    bunker_handshake: BunkerHandshakeSlot,
}

impl IdentityRuntime {
    /// Construct an identity runtime bound to a shared bunker-handshake slot.
    ///
    /// `bunker_handshake` is the `Arc<Mutex<…>>` the actor writes handshake
    /// progress into and the built-in `"bunker_handshake"` snapshot projection
    /// reads from. The two `Arc` clones share one inner `Mutex`, so an actor
    /// write is visible to the projection closure on the next tick without
    /// crossing the FFI boundary.
    pub(crate) fn new(bunker_handshake: BunkerHandshakeSlot) -> Self {
        Self {
            keys: HashMap::new(),
            remote_signers: HashMap::new(),
            order: Vec::new(),
            active: None,
            bunker_handshake,
        }
    }

    /// Write the latest bunker-handshake state into the shared projection slot
    /// (D4: actor is sole writer). A poisoned mutex recovers via
    /// `into_inner` rather than panicking the actor thread (D6).
    fn set_bunker_handshake(&self, value: Option<BunkerHandshakeDto>) {
        let mut slot = self
            .bunker_handshake
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *slot = value;
    }

    /// Test-only read of the current bunker-handshake projection state.
    ///
    /// Production code never reads this slot through the runtime — the
    /// `"bunker_handshake"` snapshot projection holds the other `Arc` clone and
    /// reads it directly. This accessor exists purely so the command-path unit
    /// tests can assert on the handshake state the actor wrote.
    #[cfg(test)]
    pub(crate) fn bunker_handshake_for_test(&self) -> Option<BunkerHandshakeDto> {
        self.bunker_handshake
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn add(&mut self, keys: Keys) -> IdentityId {
        let id = keys.public_key().to_hex();
        if !self.keys.contains_key(&id) && !self.remote_signers.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.keys.insert(id.clone(), keys);
        id
    }

    /// Register a remote-signer handle keyed by its user pubkey hex. Mirrors
    /// `add` for local keys: if the pubkey is new, append to `order`; if no
    /// account is active yet, the new remote becomes active.
    pub(crate) fn add_remote(&mut self, handle: Box<dyn RemoteSignerHandle>) -> IdentityId {
        let id = handle.pubkey_hex();
        if !self.keys.contains_key(&id) && !self.remote_signers.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.remote_signers.insert(id.clone(), handle);
        if self.active.is_none() {
            self.active = Some(id.clone());
        }
        id
    }

    fn active_keys(&self) -> Option<&Keys> {
        self.active.as_ref().and_then(|id| self.keys.get(id))
    }

    /// Borrow the active account's local `nostr::Keys`, or `None`.
    ///
    /// Returns `None` both when no account is active AND when the active
    /// account is a remote (NIP-46) signer — a remote signer holds no local
    /// secret key, so callers that need raw key material (NIP-59 gift-wrap)
    /// must surface a graceful error for that case rather than assuming a key.
    ///
    /// This is the deliberate seam for the `SendGiftWrappedDm` actor arm:
    /// `gift_wrap` requires `&Keys`, and `sign_active` (which transparently
    /// routes to a remote signer) cannot satisfy that — sealing the rumor is
    /// not a single "sign this event" operation.
    pub(crate) fn active_local_keys(&self) -> Option<&Keys> {
        self.active_keys()
    }

    fn active_remote(&self) -> Option<&dyn RemoteSignerHandle> {
        self.active
            .as_ref()
            .and_then(|id| self.remote_signers.get(id))
            .map(|b| b.as_ref())
    }

    pub(crate) fn active_pubkey(&self) -> Option<String> {
        self.active.clone()
    }

    /// Bech32-encode the active account's secret key (`nsec1…`). Returns
    /// `None` for remote signers (no local key) and when no account is active.
    pub(crate) fn active_nsec_bech32(&self) -> Option<String> {
        self.active_keys()?.secret_key().to_bech32().ok()
    }

    /// Stable signer-kind label for the active account, or `None` if no
    /// account is active. `"local"` for nsec / generated keys; whatever the
    /// remote signer returns (`"nip46"`, …) for remote handles. Exposed for
    /// the broker (Stage 4) and diagnostic-snapshot consumers; today
    /// `sync_kernel` resolves the per-row kind inline so this helper has no
    /// in-tree caller yet.
    pub(crate) fn active_signer_kind(&self) -> Option<&'static str> {
        if let Some(handle) = self.active_remote() {
            return Some(handle.signer_kind());
        }
        self.active_keys().map(|_| "local")
    }
}

/// Build an `AuthSignerFn`-shaped closure over a fixed `Keys`. Mirrors the
/// `nmp-signers::LocalKeySigner::sign_now` recipe exactly (same `nostr`
/// primitives) — kept here because D0 forbids importing `nmp-signers`.
///
/// # Correctness gates (D6 — errors become state, never silent truncation)
///
/// * **Kind range** — `unsigned.kind` is a `u32` wire type. Nostr only defines
///   kinds in `[0, 65535]` (u16 range). A value above `u16::MAX` would silently
///   wrap (e.g. 65559 → 23) without this check, publishing as the wrong kind.
///   We return `Err` so the caller surfaces a toast.
///
/// * **Malformed tags** — `Tag::parse` may reject a tag row (e.g. empty slice,
///   unknown tag type that the `nostr` crate refuses). Silent `filter_map` drops
///   are a correctness hazard for a kind-agnostic publish pass-through; a
///   protocol crate may rely on every tag it built being present in the signed
///   event. We count failures and hard-fail with a toast wording that names the
///   count so the caller can diagnose the source.
pub(super) fn sign_with(keys: &Keys, unsigned: &UnsignedEvent) -> Result<SignedEvent, String> {
    // Finding 1: validate kind is within the Nostr-defined u16 range before
    // casting. kind:65559 → kind:23 would be a silent correctness violation.
    if unsigned.kind > u16::MAX as u32 {
        return Err(format!(
            "invalid kind {}: must be in range [0, 65535]",
            unsigned.kind
        ));
    }
    let kind = Kind::from_u16(unsigned.kind as u16);

    // Finding 2: hard-fail on any malformed tag rather than silently dropping
    // it. The caller is responsible for building well-formed tags; silent
    // drops would produce a signed event that differs from the caller's intent
    // (D6 — correctness hazard for kind-agnostic publish pass-through).
    let mut tags = Vec::with_capacity(unsigned.tags.len());
    let mut malformed = 0usize;
    for t in &unsigned.tags {
        match Tag::parse(t) {
            Ok(tag) => tags.push(tag),
            Err(_) => malformed += 1,
        }
    }
    if malformed > 0 {
        return Err(format!("Dropped {malformed} malformed tag(s)"));
    }

    let event = EventBuilder::new(kind, &unsigned.content)
        .tags(tags)
        .custom_created_at(Timestamp::from(unsigned.created_at))
        .sign_with_keys(keys)
        .map_err(|e| format!("sign failed: {e}"))?;
    Ok(SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: event.kind.as_u16() as u32,
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    })
}

/// Sign `unsigned` with the active account. Returns `Err` (as state, surfaced
/// via toast — never panics across FFI, D6) if no active account. Remote
/// signers are consulted first (D0: actor only sees the trait); local keys
/// are the fallback for nsec-imported accounts.
///
/// For remote signers the call blocks the actor thread for up to
/// `REMOTE_SIGN_TIMEOUT` (5s) — bounded so a slow or crashed broker cannot
/// freeze relay ingest / subscriptions / UI for the old 45s window. On
/// timeout the `SignerError::Timeout` is `Display`-formatted into the toast
/// string (D6); a non-blocking `SignerOp::poll` path is the follow-up.
pub(crate) fn sign_active(
    identity: &IdentityRuntime,
    unsigned: &UnsignedEvent,
) -> Result<SignedEvent, String> {
    if let Some(handle) = identity.active_remote() {
        return handle
            .sign(unsigned)
            .wait(REMOTE_SIGN_TIMEOUT)
            .map_err(|e| format!("remote sign failed: {e}"));
    }
    let keys = identity
        .active_keys()
        .ok_or_else(|| "no active account — sign in first".to_string())?;
    sign_with(keys, unsigned)
}

/// Non-blocking sign with the active account.
///
/// Unlike [`sign_active`], this never blocks the actor thread. For a remote
/// (NIP-46) signer it returns the `SignerOp` verbatim — typically
/// `SignerOp::Pending`, which the caller must park (`PendingSign`) and
/// `poll()` on future loop ticks. For a local nsec/generated key the sign is
/// CPU-bound and resolves immediately into `SignerOp::Ready`.
///
/// `Err` (a `String`, surfaced as a toast per D6) covers the no-active-account
/// case; a local-signing failure is folded into a `SignerOp::Ready(Err(..))`
/// so the caller's single `poll()` match handles both signer kinds uniformly.
pub(crate) fn sign_active_nonblocking(
    identity: &IdentityRuntime,
    unsigned: &UnsignedEvent,
) -> Result<SignerOp<SignedEvent>, String> {
    if let Some(handle) = identity.active_remote() {
        return Ok(handle.sign(unsigned));
    }
    let keys = identity
        .active_keys()
        .ok_or_else(|| "no active account — sign in first".to_string())?;
    match sign_with(keys, unsigned) {
        Ok(signed) => Ok(SignerOp::ok(signed)),
        Err(e) => Ok(SignerOp::err(nmp_signer_iface::SignerError::Backend(
            format!("local sign failed: {e}"),
        ))),
    }
}

/// Bech32-encode a hex pubkey as `npub1…`. Falls back to the raw hex if the
/// pubkey doesn't parse (defensive — never panics across FFI, D6).
fn npub_from_hex(hex: &str) -> String {
    PublicKey::from_hex(hex)
        .ok()
        .and_then(|pk| pk.to_bech32().ok())
        .unwrap_or_else(|| hex.to_string())
}

/// Pre-classified human-readable label for the row's signer. Swift binds
/// this verbatim — the previous Swift-side `switch kind.lowercased() { … }`
/// (aim.md §4.4 violation) is now this Rust-side classification.
///
/// Wire tokens recognised today:
/// - `"local"` — nsec / generated key kept inside the kernel.
/// - `"nip46"` — NIP-46 bunker (remote signer).
///
/// An unknown / future token returns the token unchanged so a forward-compat
/// signer adapter can ship a custom label simply by returning a new
/// `signer_kind()` string.
fn signer_label_for_kind(kind: &str) -> String {
    match kind {
        "local" => "Local key".to_string(),
        "nip46" => "NIP-46".to_string(),
        other => other.to_string(),
    }
}

fn display_name_from_hex(id: &str) -> String {
    format!(
        "{}…{}",
        &id[..6.min(id.len())],
        &id[id.len().saturating_sub(4)..]
    )
}

/// Push the account projection + rebind the kernel's NIP-42 signer to the
/// active key (D4 single-writer: this is the only path that mutates either).
///
/// Order matters: remote signers shadow local keys for the same pubkey, so
/// the `signer_kind` projection reflects what `sign_active` will actually use.
pub(super) fn sync_kernel(identity: &IdentityRuntime, kernel: &mut Kernel) {
    let active = identity.active.clone();
    let summaries = identity
        .order
        .iter()
        .filter_map(|id| {
            let (signer_kind, npub, signer_is_remote) =
                if let Some(handle) = identity.remote_signers.get(id) {
                    (handle.signer_kind().to_string(), npub_from_hex(id), true)
                } else if let Some(keys) = identity.keys.get(id) {
                    let npub = keys.public_key().to_bech32().unwrap_or_else(|_| id.clone());
                    ("local".to_string(), npub, false)
                } else {
                    return None;
                };
            let is_active = active.as_deref() == Some(id);
            Some(AccountSummary {
                id: id.clone(),
                npub,
                display_name: display_name_from_hex(id),
                signer_label: signer_label_for_kind(&signer_kind),
                signer_kind,
                signer_is_remote,
                status: if is_active { "active" } else { "idle" }.to_string(),
                is_active,
            })
        })
        .collect::<Vec<_>>();
    kernel.set_accounts(summaries, active.clone());

    // NIP-42 auth signer binding. Remote signers (NIP-46) cannot sign NIP-42
    // challenges with the user's pubkey today — the broker's ephemeral key
    // would sign as itself, not as the user. Clear the auth signer when a
    // remote is active and rely on the broker to surface auth-required state.
    // TODO(nip46-nip42): wrap the remote signer behind AuthSignerFn so NIP-42
    // can sign through the bunker (separate follow-up — needs broker-side
    // sign_event RPC plus a sync-style adapter compatible with AuthSignerFn).
    if let Some(active_id) = active.as_ref() {
        if identity.remote_signers.contains_key(active_id) {
            kernel.clear_auth_signer();
            return;
        }
    }
    match active.as_ref().and_then(|id| identity.keys.get(id)) {
        Some(keys) => {
            let signer_keys = keys.clone();
            kernel.bind_auth_signer(
                keys.public_key().to_hex(),
                Arc::new(move |unsigned: &UnsignedEvent| sign_with(&signer_keys, unsigned)),
            );
        }
        None => kernel.clear_auth_signer(),
    }
}

/// Retarget the timeline to the active account: reuse the kernel's existing
/// `open_author` path against the active pubkey (cheap correct retarget;
/// kind:3 follow fan-out is a documented follow-up).
pub(super) fn retarget_timeline(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    match identity.active_pubkey() {
        Some(pk) => kernel.open_author(pk, relays_ready),
        None => Vec::new(),
    }
}

pub(crate) fn sign_in_nsec(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    secret: &str,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    let keys = match parse_secret(secret) {
        Some(k) => k,
        None => {
            kernel.set_last_error_toast(Some(
                "invalid secret key — expected nsec1… or 64-hex".to_string(),
            ));
            return Vec::new();
        }
    };
    let id = identity.add(keys);
    identity.active = Some(id);
    sync_kernel(identity, kernel);
    kernel.reconcile_follow_feed_after_identity_change();
    let mut outbound = kernel.active_account_bootstrap_requests();
    outbound.extend(retarget_timeline(identity, kernel, relays_ready));
    outbound
}

/// Pubkeys every fresh account follows out-of-the-box (hex, kind:3).
pub(super) const DEFAULT_FOLLOWS: &[&str] = &[
    // npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52",
    // fiatjaf
    "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
];
const DEFAULT_ONBOARDING_RELAYS: &[(&str, &str)] = &[
    ("wss://relay.primal.net", "both,indexer"),
    ("wss://purplepag.es", "indexer"),
];
const DEFAULT_ONBOARDING_OVERRIDE_ROLE: &str = "both,indexer";

pub(crate) fn create_account(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    relays_ready: bool,
    profile: &HashMap<String, String>,
    relays: &[(String, String)],
    _mls: bool,
) -> Vec<OutboundMessage> {
    let id = identity.add(Keys::generate());
    identity.active = Some(id.clone());
    sync_kernel(identity, kernel);
    let relay_rows = relay_rows_from_create_account(relays);
    kernel.set_relay_edit_rows(relay_rows.clone());

    // Pre-populate seed_contacts so the follow-feed can be set up immediately
    // without waiting for the published kind:3 to round-trip from relays.
    let follows = DEFAULT_FOLLOWS
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    kernel.prepopulate_seed_contacts(id.clone(), follows);

    let mut publish_outbound = Vec::new();

    // ── Publish kind:0 metadata ──────────────────────────────────
    let kind0_content = match serde_json::to_string(profile) {
        Ok(json) => json,
        Err(e) => {
            kernel.set_last_error_toast(Some(format!("profile serialisation: {}", e)));
            String::new()
        }
    };
    if let (false, Some(author)) = (kind0_content.is_empty(), identity.active_pubkey()) {
        let unsigned_meta = UnsignedEvent {
            pubkey: author,
            kind: 0,
            tags: Vec::new(),
            content: kind0_content,
            created_at: kernel.now_secs(),
        };
        if let Ok(signed) = sign_active(identity, &unsigned_meta) {
            // Cold-start routing (same chicken-and-egg as kind:10002 below).
            // A brand-new account has no kind:10002 on file, so the NIP-65
            // outbox resolver (`PublishTarget::Auto`) would resolve
            // `NoTargets` and the publish engine would silently drop this
            // profile metadata — nobody would ever see the new account's
            // display name. Route the initial kind:0 to the explicit
            // cold-start target instead.
            let target_relays = cold_start_publish_targets(kernel, &relay_rows);
            if target_relays.is_empty() {
                // D6: no usable cold-start relay — surface a toast, never
                // panic. The account still exists locally; the user can add
                // relays and re-publish their profile from Settings.
                kernel.set_last_error_toast(Some(
                    "could not publish profile — no cold-start relays available".to_string(),
                ));
            } else {
                publish_outbound.extend(kernel.publish_signed_to(
                    &signed,
                    &[],
                    crate::publish::PublishTarget::Explicit {
                        relays: target_relays,
                    },
                ));
            }
        }
    }

    // ── Publish kind:10002 relay list ─────────────────────────────
    let relay_tags = nip65_tags_from_relay_rows(&relay_rows);
    if let (false, Some(author)) = (relay_tags.is_empty(), identity.active_pubkey()) {
        let unsigned_relay = UnsignedEvent {
            pubkey: author,
            kind: 10002,
            tags: relay_tags,
            content: String::new(),
            created_at: kernel.now_secs(),
        };
        if let Ok(signed) = sign_active(identity, &unsigned_relay) {
            kernel.prepopulate_author_relay_list(
                signed.unsigned.pubkey.clone(),
                signed.id.clone(),
                signed.unsigned.created_at,
                signed.unsigned.tags.clone(),
            );
            // Cold-start routing. A brand-new account has no kind:10002 on
            // file yet, so the NIP-65 outbox resolver (`PublishTarget::Auto`)
            // would resolve `NoTargets` and the publish engine would silently
            // drop this very event — the chicken-and-egg the account can never
            // escape (it can't announce its relays because it has no relays on
            // record). Route the initial relay list explicitly instead: to the
            // relays the user just declared (the canonical NIP-65 home of a
            // relay list — publish it to the relays it names) unioned with the
            // well-known discovery seed so others can find the new account.
            let target_relays = cold_start_publish_targets(kernel, &relay_rows);
            if target_relays.is_empty() {
                // D6: no usable cold-start relay — surface a toast, never
                // panic. The account still exists locally; the user can add
                // relays and re-publish from Settings.
                kernel.set_last_error_toast(Some(
                    "could not publish relay list — no cold-start relays available".to_string(),
                ));
            } else {
                publish_outbound.extend(kernel.publish_signed_to(
                    &signed,
                    &[],
                    crate::publish::PublishTarget::Explicit {
                        relays: target_relays,
                    },
                ));
            }
        }
    }

    kernel.reconcile_follow_feed_after_identity_change();
    let mut outbound = kernel.active_account_bootstrap_requests();
    outbound.extend(retarget_timeline(identity, kernel, relays_ready));
    outbound.extend(publish_outbound);
    outbound.extend(publish_initial_follows(identity, kernel, &relay_rows));
    outbound
}

pub(crate) fn ensure_default_onboarding_relays(kernel: &mut Kernel) {
    if kernel.relay_edit_rows_snapshot().is_empty() {
        kernel.set_relay_edit_rows(relay_rows_from_create_account(&[]));
    }
}

/// Resolve the explicit relay set every *initial* event a brand-new account
/// emits — kind:0 (profile metadata), kind:3 (contacts) and kind:10002 (relay
/// list) — is published to on account creation (cold-start).
///
/// A freshly-created account has no kind:10002 in the store, so the NIP-65
/// outbox resolver cannot route any of its first events — it would resolve
/// `NoTargets` and the publish engine would drop them. This helper builds the
/// explicit cold-start target instead:
///
/// 1. The canonical relay rows the user just declared during onboarding; and
/// 2. The kernel's well-known discovery seed (`bootstrap_discovery_relays`) so
///    other clients performing relay-list / profile discovery can find the new
///    account.
///
/// The result is sorted + deduped. It is empty only when the user supplied no
/// relays AND no discovery relays are configured — the caller treats an empty
/// result as a D6 graceful failure (toast, never panic).
///
/// This applies ONLY to cold-start: `create_account` is the sole caller, and a
/// brand-new account by construction has no prior kind:10002. A user updating
/// their profile / contacts / relay list later publishes through
/// `publish_signed` (`Auto`), which routes to their already-declared write
/// relays — that path is unaffected.
fn cold_start_publish_targets(kernel: &Kernel, relay_rows: &[RelayEditRow]) -> Vec<String> {
    let mut targets: Vec<String> = relay_rows
        .iter()
        .map(|row| row.url.clone())
        .chain(kernel.bootstrap_discovery_relays())
        .collect();
    targets.sort();
    targets.dedup();
    targets
}

fn relay_rows_from_create_account(relays: &[(String, String)]) -> Vec<RelayEditRow> {
    let source = if relays.is_empty() {
        DEFAULT_ONBOARDING_RELAYS
            .iter()
            .map(|(url, role)| ((*url).to_string(), (*role).to_string()))
            .collect::<Vec<_>>()
    } else {
        relays.to_vec()
    };
    source
        .iter()
        .filter_map(|(url, role)| {
            let url = canonical_relay_url(url)?;
            let raw_role = if role.trim().is_empty() {
                DEFAULT_ONBOARDING_OVERRIDE_ROLE
            } else {
                role
            };
            let role = canonical_relay_role(raw_role).unwrap_or_else(|| "both".to_string());
            Some(RelayEditRow { url, role })
        })
        .collect()
}

fn nip65_tags_from_relay_rows(rows: &[RelayEditRow]) -> Vec<Vec<String>> {
    rows.iter()
        .filter_map(|row| {
            let read = has_role(&row.role, "read");
            let write = has_role(&row.role, "write");
            match (read, write) {
                (true, true) => Some(vec!["r".to_string(), row.url.clone()]),
                (true, false) => Some(vec!["r".to_string(), row.url.clone(), "read".to_string()]),
                (false, true) => Some(vec!["r".to_string(), row.url.clone(), "write".to_string()]),
                (false, false) => None,
            }
        })
        .collect()
}

/// Publish the cold-start kind:3 contacts list (`DEFAULT_FOLLOWS`) for a
/// brand-new account.
///
/// Like kind:0 and kind:10002, this is a cold-start publish: the account has
/// no kind:10002 on file, so the NIP-65 outbox resolver (`PublishTarget::Auto`)
/// would resolve `NoTargets` and the publish engine would silently drop the
/// contacts list — the new account's follows would never propagate. The
/// initial kind:3 is therefore routed to the explicit cold-start target
/// (`cold_start_publish_targets`), the same union of declared + discovery
/// relays the initial kind:0 / kind:10002 use.
///
/// `relay_rows` are the canonical relay rows declared during onboarding,
/// threaded through from `create_account` so the cold-start target can be
/// resolved without rebuilding or re-normalizing them.
fn publish_initial_follows(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    relay_rows: &[RelayEditRow],
) -> Vec<OutboundMessage> {
    let Some(author) = identity.active_pubkey() else {
        return Vec::new();
    };
    let tags = DEFAULT_FOLLOWS
        .iter()
        .map(|p| vec!["p".to_string(), p.to_string()])
        .collect::<Vec<_>>();
    let unsigned = UnsignedEvent {
        pubkey: author,
        kind: 3,
        tags,
        content: String::new(),
        created_at: kernel.now_secs(),
    };
    match sign_active(identity, &unsigned) {
        Ok(signed) => {
            let target_relays = cold_start_publish_targets(kernel, relay_rows);
            if target_relays.is_empty() {
                // D6: no usable cold-start relay — surface a toast, never
                // panic. The follow set is already pre-populated locally
                // (`prepopulate_seed_contacts`); the user can re-publish
                // their contacts once relays are configured.
                kernel.set_last_error_toast(Some(
                    "could not publish contacts — no cold-start relays available".to_string(),
                ));
                Vec::new()
            } else {
                kernel.publish_signed_to(
                    &signed,
                    &[],
                    crate::publish::PublishTarget::Explicit {
                        relays: target_relays,
                    },
                )
            }
        }
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            Vec::new()
        }
    }
}

pub(crate) fn switch_active(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    identity_id: &str,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    if !identity.keys.contains_key(identity_id)
        && !identity.remote_signers.contains_key(identity_id)
    {
        kernel.set_last_error_toast(Some(format!("account not found: {identity_id}")));
        return Vec::new();
    }
    if identity.active.as_deref() == Some(identity_id) {
        return Vec::new();
    }
    identity.active = Some(identity_id.to_string());
    sync_kernel(identity, kernel);
    // #168: reconcile the M2 follow-feed to the NEW active account — withdraw
    // the prior account's follow interests + emit the CLOSE diff (stale-feed /
    // privacy leak fix). Runs AFTER sync_kernel set kernel.active_account.
    kernel.reconcile_follow_feed_after_identity_change();
    let mut outbound = kernel.active_account_bootstrap_requests();
    outbound.extend(retarget_timeline(identity, kernel, relays_ready));
    outbound
}

pub(crate) fn remove_account(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    identity_id: &str,
) -> Vec<OutboundMessage> {
    let had_local = identity.keys.remove(identity_id).is_some();
    let had_remote = match identity.remote_signers.remove(identity_id) {
        Some(handle) => {
            // Drain in-flight requests before dropping so blocked callers
            // fail fast rather than waiting for the remote-sign timeout.
            handle.disconnect();
            drop(handle);
            true
        }
        None => false,
    };
    if !had_local && !had_remote {
        return Vec::new();
    }
    identity.order.retain(|x| x != identity_id);
    if identity.active.as_deref() == Some(identity_id) {
        identity.active = identity.order.first().cloned();
    }
    sync_kernel(identity, kernel);
    // #168: removing an account (esp. the last → active=None) must withdraw
    // the prior account's M2 follow interests + emit the CLOSE diff so the
    // follow-feed subs do not leak past logout. Runs AFTER sync_kernel.
    kernel.reconcile_follow_feed_after_identity_change();
    Vec::new()
}

/// Broker → actor: register a fully-handshaken remote signer (e.g. completed
/// NIP-46 bunker handshake). Becomes active if no account was active; pushes
/// a snapshot update + timeline retarget so the UI swaps immediately.
pub(crate) fn add_remote_signer(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    handle: Box<dyn RemoteSignerHandle>,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    let _id = identity.add_remote(handle);
    sync_kernel(identity, kernel);
    retarget_timeline(identity, kernel, relays_ready)
}

/// Broker → actor: latest NIP-46 handshake progress. Stage `"idle"` clears
/// the projection; everything else replaces it.
///
/// D0: the handshake state is an app noun, so it is written to the shared
/// [`BunkerHandshakeSlot`] (read by the `"bunker_handshake"` snapshot
/// projection) instead of a typed `KernelSnapshot` field. The slot write does
/// NOT flip `changed_since_emit`, so the kernel is marked dirty explicitly —
/// otherwise the refreshed projection could sit unemitted until an unrelated
/// kernel mutation triggered a tick.
pub(crate) fn bunker_handshake_progress(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    stage: String,
    message: Option<String>,
) {
    let value = if stage == "idle" {
        None
    } else {
        Some(BunkerHandshakeDto { stage, message })
    };
    identity.set_bunker_handshake(value);
    kernel.mark_changed_since_emit();
}

pub(crate) fn sign_in_bunker(identity: &IdentityRuntime, kernel: &mut Kernel, uri: &str) {
    // Stage 3 of NIP-46 wiring: actor exposes handshake-progress snapshot.
    // Stage 4 of NIP-46 wiring: actor delegates the handshake to a broker
    // registered via `crate::bunker_hook::register_bunker_hook`.
    //
    // Here we shape-validate the URI, seed the snapshot with `"connecting"`
    // so the host sign-in flow renders progress immediately, then hand
    // the URI to the registered broker. The broker drives the connect /
    // get_public_key dance on its own thread and reports progress via
    // `BunkerHandshakeProgress` + `AddRemoteSigner`. D0 stays clean —
    // `nmp-signers` is still NOT imported in `nmp-core`; the broker crate
    // (`nmp-signer-broker`) is the only place that links both sides.
    if parse_bunker_remote(uri).is_none() {
        kernel.set_last_error_toast(Some(
            "invalid bunker:// URI — expected bunker://<64-hex-pubkey>?relay=…".to_string(),
        ));
        return;
    }
    identity.set_bunker_handshake(Some(BunkerHandshakeDto {
        stage: "connecting".to_string(),
        message: Some("Waiting for broker...".to_string()),
    }));
    kernel.mark_changed_since_emit();
    if !crate::bunker_hook::invoke_bunker_connect_hook(uri) {
        // Defence against init-order bugs: the broker should be registered
        // before any URI can reach the actor. If it isn't, surface a clear
        // toast and clear the progress projection (D6 — error becomes state,
        // never panic across FFI).
        identity.set_bunker_handshake(None);
        kernel.set_last_error_toast(Some(
            "NIP-46 broker not initialised — call nmp_signer_broker_init".to_string(),
        ));
    }
}

pub(crate) fn restore_bunker_session(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    payload_json: &str,
) {
    identity.set_bunker_handshake(Some(BunkerHandshakeDto {
        stage: "connecting".to_string(),
        message: Some("Restoring broker session...".to_string()),
    }));
    kernel.mark_changed_since_emit();
    if !crate::bunker_hook::invoke_bunker_restore_hook(payload_json) {
        identity.set_bunker_handshake(None);
        kernel.set_last_error_toast(Some(
            "NIP-46 broker not initialised — call nmp_signer_broker_init".to_string(),
        ));
    }
}

/// Parse an nsec/bech32 or 64-hex secret into `Keys`. `None` on bad input.
fn parse_secret(secret: &str) -> Option<Keys> {
    let s = secret.trim();
    if let Ok(sk) = SecretKey::from_bech32(s) {
        return Some(Keys::new(sk));
    }
    if s.len() == 64 {
        if let Ok(sk) = SecretKey::from_hex(s) {
            return Some(Keys::new(sk));
        }
    }
    None
}

/// Minimal `bunker://<remote-pubkey-hex>?relay=…` shape check. Returns the
/// remote pubkey hex if the URI is well-formed.
fn parse_bunker_remote(uri: &str) -> Option<String> {
    let rest = uri.trim().strip_prefix("bunker://")?;
    let pubkey = rest.split(['?', '/']).next()?;
    if pubkey.len() == 64 && pubkey.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(pubkey.to_string())
    } else {
        None
    }
}
