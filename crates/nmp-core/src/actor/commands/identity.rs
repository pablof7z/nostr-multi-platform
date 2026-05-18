//! Actor-local identity runtime + sign-in / switch / remove handlers.
//!
//! D4: the actor thread is the single writer of identity facts. The
//! authoritative store is the `HashMap<IdentityId, Keys>` here; the kernel's
//! `accounts` projection is pushed via `Kernel::set_accounts` after every
//! mutation, then emitted.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::{EventBuilder, Keys, Kind, SecretKey, Tag, Timestamp};

use crate::kernel::{AccountSummary, Kernel};
use crate::relay::OutboundMessage;
use crate::substrate::{SignedEvent, UnsignedEvent};

/// IdentityId is the hex pubkey (matches NDK / applesauce / `AccountManager`).
pub(crate) type IdentityId = String;

/// Actor-local multi-account state. Insertion-ordered for deterministic UI.
pub(crate) struct IdentityRuntime {
    keys: HashMap<IdentityId, Keys>,
    order: Vec<IdentityId>,
    active: Option<IdentityId>,
}

impl IdentityRuntime {
    pub(crate) fn new() -> Self {
        Self {
            keys: HashMap::new(),
            order: Vec::new(),
            active: None,
        }
    }

    fn add(&mut self, keys: Keys) -> IdentityId {
        let id = keys.public_key().to_hex();
        if !self.keys.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.keys.insert(id.clone(), keys);
        id
    }

    fn active_keys(&self) -> Option<&Keys> {
        self.active.as_ref().and_then(|id| self.keys.get(id))
    }

    pub(crate) fn active_pubkey(&self) -> Option<String> {
        self.active.clone()
    }
}

/// Build an `AuthSignerFn`-shaped closure over a fixed `Keys`. Mirrors the
/// `nmp-signers::LocalKeySigner::sign_now` recipe exactly (same `nostr`
/// primitives) — kept here because D0 forbids importing `nmp-signers`.
fn sign_with(keys: &Keys, unsigned: &UnsignedEvent) -> Result<SignedEvent, String> {
    let kind = Kind::from_u16(unsigned.kind as u16);
    let tags = unsigned
        .tags
        .iter()
        .filter_map(|t| Tag::parse(t).ok())
        .collect::<Vec<_>>();
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

/// Sign `unsigned` with the active account's keys. Returns `Err` (as state,
/// surfaced via toast — never panics across FFI, D6) if no active account.
pub(crate) fn sign_active(
    identity: &IdentityRuntime,
    unsigned: &UnsignedEvent,
) -> Result<SignedEvent, String> {
    let keys = identity
        .active_keys()
        .ok_or_else(|| "no active account — sign in first".to_string())?;
    sign_with(keys, unsigned)
}

/// Push the account projection + rebind the kernel's NIP-42 signer to the
/// active key (D4 single-writer: this is the only path that mutates either).
fn sync_kernel(identity: &IdentityRuntime, kernel: &mut Kernel) {
    let active = identity.active.clone();
    let summaries = identity
        .order
        .iter()
        .filter_map(|id| {
            let keys = identity.keys.get(id)?;
            let npub = keys
                .public_key()
                .to_bech32()
                .unwrap_or_else(|_| id.clone());
            Some(AccountSummary {
                id: id.clone(),
                npub,
                display_name: format!("{}…{}", &id[..6.min(id.len())], &id[id.len().saturating_sub(4)..]),
                signer_kind: "local".to_string(),
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
fn retarget_timeline(
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
    retarget_timeline(identity, kernel, relays_ready)
}

pub(crate) fn create_account(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    let id = identity.add(Keys::generate());
    identity.active = Some(id);
    sync_kernel(identity, kernel);
    retarget_timeline(identity, kernel, relays_ready)
}

pub(crate) fn switch_active(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    identity_id: &str,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    if !identity.keys.contains_key(identity_id) {
        kernel.set_last_error_toast(Some(format!("account not found: {identity_id}")));
        return Vec::new();
    }
    if identity.active.as_deref() == Some(identity_id) {
        return Vec::new();
    }
    identity.active = Some(identity_id.to_string());
    sync_kernel(identity, kernel);
    retarget_timeline(identity, kernel, relays_ready)
}

pub(crate) fn remove_account(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    identity_id: &str,
) -> Vec<OutboundMessage> {
    if identity.keys.remove(identity_id).is_none() {
        return Vec::new();
    }
    identity.order.retain(|x| x != identity_id);
    if identity.active.as_deref() == Some(identity_id) {
        identity.active = identity.order.first().cloned();
    }
    sync_kernel(identity, kernel);
    Vec::new()
}

pub(crate) fn sign_in_bunker(kernel: &mut Kernel, uri: &str) {
    // Shape-validate the bunker URI without wiring transport (D0: Nip46Signer
    // lives in nmp-signers; importing it would be a dependency cycle). The
    // build doc §11 authorizes nsec-only multi-account for this build.
    if parse_bunker_remote(uri).is_some() {
        kernel.set_last_error_toast(Some(
            "valid bunker:// URI parsed, but NIP-46 transport is not wired in \
             this build — use nsec sign-in for this session"
                .to_string(),
        ));
    } else {
        kernel.set_last_error_toast(Some(
            "invalid bunker:// URI — expected bunker://<64-hex-pubkey>?relay=…"
                .to_string(),
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
