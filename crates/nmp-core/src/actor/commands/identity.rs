//! Actor-local identity runtime + sign-in / switch / remove handlers.
//!
//! D4: the actor thread is the single writer of identity facts. The
//! authoritative store is the `HashMap<IdentityId, Keys>` here; the kernel's
//! `accounts` projection is pushed via `Kernel::set_accounts` after every
//! mutation, then emitted.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::{EventBuilder, Keys, Kind, PublicKey, SecretKey, Tag, Timestamp};

use crate::kernel::{AccountSummary, BunkerHandshakeDto, Kernel};
use crate::relay::OutboundMessage;
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::{SignedEvent, UnsignedEvent};

/// `SignerOp::wait` timeout for remote-signer signs. Bunker UX can require a
/// user tap on the phone — long enough to cover that, short enough that a
/// crashed broker doesn't wedge the actor indefinitely. The publish callsites
/// (`publish.rs`) surface the error as `last_error_toast` per D6.
const REMOTE_SIGN_TIMEOUT: Duration = Duration::from_secs(45);

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
}

impl IdentityRuntime {
    pub(crate) fn new() -> Self {
        Self {
            keys: HashMap::new(),
            remote_signers: HashMap::new(),
            order: Vec::new(),
            active: None,
        }
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

    /// Drop the remote signer (if any) for `identity_id`. If it was active,
    /// fall back to the next account in `order` (mirroring `remove_account`).
    pub(crate) fn remove_remote(&mut self, identity_id: &str) {
        if self.remote_signers.remove(identity_id).is_none() {
            return;
        }
        // Only drop from `order` if no local key for the same pubkey survives.
        if !self.keys.contains_key(identity_id) {
            self.order.retain(|x| x != identity_id);
        }
        if self.active.as_deref() == Some(identity_id) {
            self.active = self.order.first().cloned();
        }
    }

    fn active_keys(&self) -> Option<&Keys> {
        self.active.as_ref().and_then(|id| self.keys.get(id))
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

    /// Stable signer-kind label for the active account, or `None` if no
    /// account is active. `"local"` for nsec / generated keys; whatever the
    /// remote signer returns (`"nip46"`, …) for remote handles. Exposed for
    /// the broker (Stage 4) and diagnostic-snapshot consumers; today
    /// `sync_kernel` resolves the per-row kind inline so this helper has no
    /// in-tree caller yet.
    #[allow(dead_code)]
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

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Sign `unsigned` with the active account. Returns `Err` (as state, surfaced
/// via toast — never panics across FFI, D6) if no active account. Remote
/// signers are consulted first (D0: actor only sees the trait); local keys
/// are the fallback for nsec-imported accounts.
///
/// For remote signers the call blocks the actor thread for up to
/// `REMOTE_SIGN_TIMEOUT` (45s) — long enough for NIP-46 user-approval UX,
/// short enough that a crashed broker doesn't wedge the actor forever.
/// `SignerError` is `Display`-formatted into the toast string.
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

/// Bech32-encode a hex pubkey as `npub1…`. Falls back to the raw hex if the
/// pubkey doesn't parse (defensive — never panics across FFI, D6).
fn npub_from_hex(hex: &str) -> String {
    PublicKey::from_hex(hex)
        .ok()
        .and_then(|pk| pk.to_bech32().ok())
        .unwrap_or_else(|| hex.to_string())
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
            let (signer_kind, npub) = if let Some(handle) = identity.remote_signers.get(id) {
                (handle.signer_kind().to_string(), npub_from_hex(id))
            } else if let Some(keys) = identity.keys.get(id) {
                let npub = keys.public_key().to_bech32().unwrap_or_else(|_| id.clone());
                ("local".to_string(), npub)
            } else {
                return None;
            };
            Some(AccountSummary {
                id: id.clone(),
                npub,
                display_name: display_name_from_hex(id),
                signer_kind,
                status: if active.as_deref() == Some(id) {
                    "active"
                } else {
                    "idle"
                }
                .to_string(),
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

pub(crate) fn create_account(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    let id = identity.add(Keys::generate());
    identity.active = Some(id);
    sync_kernel(identity, kernel);
    kernel.reconcile_follow_feed_after_identity_change();
    let mut outbound = kernel.active_account_bootstrap_requests();
    outbound.extend(retarget_timeline(identity, kernel, relays_ready));
    outbound
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
    let had_remote = identity.remote_signers.remove(identity_id).is_some();
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

/// Broker → actor: drop a remote signer by user pubkey hex. If it was the
/// active account, fall back to the next account in `order`.
pub(crate) fn remove_remote_signer(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    identity_id: &str,
) -> Vec<OutboundMessage> {
    if !identity.remote_signers.contains_key(identity_id) {
        return Vec::new();
    }
    identity.remove_remote(identity_id);
    sync_kernel(identity, kernel);
    // #168: same reconcile as remove_account — a removed remote signer that
    // was the active account must not leave its follow-feed interests live.
    kernel.reconcile_follow_feed_after_identity_change();
    Vec::new()
}

/// Broker → actor: latest NIP-46 handshake progress. Stage `"idle"` clears
/// the projection; everything else replaces it (snapshot diff handled by the
/// setter — no emit if unchanged).
pub(crate) fn bunker_handshake_progress(
    kernel: &mut Kernel,
    stage: String,
    message: Option<String>,
) {
    let value = if stage == "idle" {
        None
    } else {
        Some(BunkerHandshakeDto { stage, message })
    };
    kernel.set_bunker_handshake(value);
}

pub(crate) fn sign_in_bunker(kernel: &mut Kernel, uri: &str) {
    // Stage 3 of NIP-46 wiring: actor exposes handshake-progress snapshot.
    // Stage 4 of NIP-46 wiring: actor delegates the handshake to a broker
    // registered via `crate::bunker_hook::register_bunker_hook`.
    //
    // Here we shape-validate the URI, seed the snapshot with `"connecting"`
    // so the SwiftUI sign-in flow renders progress immediately, then hand
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
    kernel.set_bunker_handshake(Some(BunkerHandshakeDto {
        stage: "connecting".to_string(),
        message: Some("Waiting for broker...".to_string()),
    }));
    if !crate::bunker_hook::invoke_bunker_hook(uri) {
        // Defence against init-order bugs: the broker should be registered
        // before any URI can reach the actor. If it isn't, surface a clear
        // toast and clear the progress projection (D6 — error becomes state,
        // never panic across FFI).
        kernel.set_bunker_handshake(None);
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
