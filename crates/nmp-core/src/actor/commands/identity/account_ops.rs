//! Account management command handlers — add_signer, switch_active,
//! remove_account, and kernel sync helpers.
//! `create_account` lives in the sibling `create_account` module.

use std::sync::Arc;

use nostr::nips::nip19::ToBech32;
use nostr::Keys;
use nostr::PublicKey;

use crate::kernel::{AccountSummary, Kernel};
use crate::relay::OutboundMessage;
use nmp_signer_iface::UnsignedEvent;

use super::runtime::{IdentityId, IdentityRuntime};
use super::sign::sign_with;

/// Push the account projection + rebind the kernel's NIP-42 signer to the
/// active key (D4 single-writer: this is the only path that mutates either).
///
/// Order matters: remote signers shadow local keys for the same pubkey, so
/// the `signer_kind` projection reflects what `sign_active_nonblocking` will
/// actually use.
pub(crate) fn sync_kernel(identity: &IdentityRuntime, kernel: &mut Kernel) {
    let active = identity.active.clone();
    let summaries = identity
        .order
        .iter()
        .filter(|id| !identity.app_managed.contains(*id))
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
                // aim.md §2 — no `short_pubkey` placeholder; `None` until
                // kind:0 lands, presentation layer renders its own
                // fallback. `Kernel::accounts_enriched` populates this
                // once kind:0 arrives.
                display_name: None,
                signer_kind,
                signer_is_remote,
                status: if is_active { "active" } else { "idle" }.to_string(),
                is_active,
                picture_url: None,
            })
        })
        .collect::<Vec<_>>();
    kernel.set_accounts(summaries, active.clone());

    // NIP-42 auth signer binding (V-06 / #960 — ONE uniform async sign seam).
    //
    // A REMOTE signer (NIP-46 / NIP-55) cannot sign synchronously — only the
    // broker holds the key — so we bind the AUTH *pubkey* (the active id is the
    // signer pubkey hex) and let `handle_auth_challenge` PARK the kind:22242 for
    // the async signer port. A LOCAL key binds the synchronous `AuthSignerFn`
    // and resolves inline. The kernel keeps these two bindings disjoint. No more
    // remote bail / "bunker AUTH unsupported" toast — bunker accounts now pass
    // NIP-42 AUTH gates as themselves.
    if let Some(active_id) = active.as_ref() {
        if identity.remote_signers.contains_key(active_id) {
            kernel.bind_auth_remote(active_id.clone());
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

/// Retarget the timeline to the active account.
pub(crate) fn retarget_timeline(
    identity: &IdentityRuntime,
    _kernel: &mut Kernel,
    _relays_ready: bool,
) -> Vec<OutboundMessage> {
    let _ = identity; // keep for callers that pass it; no retarget needed here
    Vec::new()
}

/// Bech32-encode a hex pubkey as `npub1…`. Falls back to the raw hex if the
/// pubkey doesn't parse (defensive — never panics across FFI, D6).
fn npub_from_hex(hex: &str) -> String {
    PublicKey::from_hex(hex)
        .ok()
        .and_then(|pk| pk.to_bech32().ok())
        .unwrap_or_else(|| hex.to_string())
}

/// Unified sign-in reducer for local, remote, and app-managed signer sources.
pub(crate) fn add_signer(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    source: crate::actor::SignerSource,
    make_active: bool,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    match source {
        crate::actor::SignerSource::LocalNsec(secret) => {
            add_local_signer(identity, kernel, secret, make_active, relays_ready, false)
        }
        crate::actor::SignerSource::AppManagedLocalNsec(secret) => {
            add_local_signer(identity, kernel, secret, make_active, relays_ready, true)
        }
        crate::actor::SignerSource::BunkerUri(uri) => {
            start_bunker_signer(identity, kernel, &uri, make_active)
        }
        crate::actor::SignerSource::RemoteHandle(handle) => {
            add_remote_signer_handle(identity, kernel, handle, make_active, relays_ready)
        }
    }
}

fn add_local_signer(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    secret: zeroize::Zeroizing<String>,
    make_active: bool,
    relays_ready: bool,
    app_managed: bool,
) -> Vec<OutboundMessage> {
    let Some(keys) = parse_secret(secret.as_str()) else {
        kernel.set_last_error_toast(Some(
            "invalid secret key — expected nsec1… or 64-hex".to_string(),
        ));
        return Vec::new();
    };
    let id = identity.add(keys);
    identity.set_app_managed(&id, app_managed);
    finish_signer_add(
        identity,
        kernel,
        id,
        make_active && !app_managed,
        relays_ready,
    )
}

fn start_bunker_signer(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    uri: &str,
    make_active: bool,
) -> Vec<OutboundMessage> {
    identity.pending_bunker_make_active = make_active;
    super::signer_state::start_bunker_handshake(identity, kernel, uri);
    Vec::new()
}

fn add_remote_signer_handle(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    handle: Box<dyn nmp_signer_iface::RemoteSignerHandle>,
    make_active: bool,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    let id = identity.add_remote_inactive(handle);
    let stashed_make_active = std::mem::take(&mut identity.pending_bunker_make_active);
    identity.set_app_managed(&id, false);
    let should_activate = make_active || stashed_make_active || identity.active.is_none();
    finish_signer_add(identity, kernel, id, should_activate, relays_ready)
}

pub(super) fn finish_signer_add(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    id: IdentityId,
    should_activate: bool,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    if should_activate {
        identity.active = Some(id);
    }
    sync_kernel(identity, kernel);
    if should_activate {
        kernel.reconcile_follow_feed_after_identity_change();
        let mut outbound = kernel.active_account_bootstrap_requests();
        outbound.extend(retarget_timeline(identity, kernel, relays_ready));
        outbound
    } else {
        Vec::new()
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
    if identity.is_app_managed(identity_id) {
        kernel.set_last_error_toast(Some(format!(
            "account is app-managed and cannot be made active: {identity_id}"
        )));
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
    identity.app_managed.remove(identity_id);
    if identity.active.as_deref() == Some(identity_id) {
        identity.active = identity
            .order
            .iter()
            .find(|id| !identity.is_app_managed(id))
            .cloned();
    }
    sync_kernel(identity, kernel);
    // #168: removing an account (esp. the last → active=None) must withdraw
    // the prior account's M2 follow interests + emit the CLOSE diff so the
    // follow-feed subs do not leak past logout. Runs AFTER sync_kernel.
    kernel.reconcile_follow_feed_after_identity_change();
    Vec::new()
}

/// Parse an nsec/bech32 or 64-hex secret into `Keys`. `None` on bad input.
fn parse_secret(secret: &str) -> Option<Keys> {
    use nostr::nips::nip19::FromBech32;
    use nostr::SecretKey;
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
