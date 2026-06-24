//! Cold-start account creation — key generation, initial event publish
//! (kind:0 / kind:3 / kind:10002), relay row canonicalisation.

use std::collections::HashMap;

use nostr::Keys;

use crate::actor::{canonical_relay_role, has_role};
use crate::kernel::{AppRelay, Kernel};
use crate::relay::{canonical_relay_url, OutboundMessage};
use nmp_signer_iface::UnsignedEvent;
use crate::util::sort_dedup;

use super::account_ops::{retarget_timeline, sync_kernel};
use super::runtime::IdentityRuntime;
use super::sign::sign_active_nonblocking;

const DEFAULT_ONBOARDING_OVERRIDE_ROLE: &str = "both,indexer";

pub(crate) fn create_account(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    relays_ready: bool,
    profile: &HashMap<String, String>,
    relays: &[(String, String)],
    initial_follows: &[String],
    _mls: bool,
    make_active: bool,
) -> Vec<OutboundMessage> {
    let id = identity.add(Keys::generate());
    if make_active {
        identity.active = Some(id.clone());
    }
    sync_kernel(identity, kernel);
    let relay_rows = relay_rows_from_create_account(relays);
    if !relays.is_empty() {
        kernel.set_configured_relays(relay_rows.clone());
    }

    // Operator policy (which accounts a fresh account auto-follows) is supplied
    // by the app, never hardcoded in NMP (the old `DEFAULT_FOLLOWS` const lived
    // here and baked Chirp's seed pubkeys into the framework — #1493). An empty
    // `initial_follows` means the account starts with no contacts and no
    // cold-start kind:3 is published.
    //
    // Seed the contacts cache with the (possibly empty) known follow set so the
    // account's contact list is recorded as KNOWN rather than UNSYNCED. A
    // brand-new local account has, by construction, no REMOTE kind:3 to wait
    // for — its empty follow set is authoritative immediately. This is the
    // signal the `follow` / `follow_many` fail-closed gate
    // (`Kernel::try_current_kind3_event_for_edit`) reads to distinguish a fresh
    // local account (safe to publish its FIRST kind:3, e.g. when onboarding
    // applies follow packs after account creation) from an EXISTING account
    // whose remote kind:3 has not synced (must fail closed to avoid clobbering
    // it). Seeding the cache publishes nothing — #1493 still holds: an empty
    // `initial_follows` emits no cold-start kind:3 and NMP hardcodes no follows.
    kernel.prepopulate_contacts(id.clone(), initial_follows.to_vec());

    let mut publish_outbound = Vec::new();
    // ── Publish kind:0 metadata ──────────────────────────────────
    let kind0_content = match serde_json::to_string(profile) {
        Ok(json) => json,
        Err(e) => {
            kernel.set_last_error_toast(Some(format!("profile serialisation: {e}")));
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
        // V-54 (closed, non-bug) / ADR-0040 site-3 correction: `create_account`
        // activates a fresh LOCAL key before this sign, so `sign_active_nonblocking`
        // takes the synchronous Ready branch — no remote round-trip, no actor
        // stall (D8). Enforce that invariant so a future edit can't silently
        // reintroduce an onboarding freeze (V-111 / #972 removed the blocking
        // primitive entirely).
        debug_assert!(
            identity.active_remote().is_none(),
            "cold-start kind:0 sign must run with a local key active (else blocks the actor)"
        );
        match sign_active_nonblocking(identity, &unsigned_meta).and_then(|mut op| {
            op.poll()
                .ok_or_else(|| "sign op pending — remote signer on cold-start path".to_string())
                .and_then(|r| r.map_err(|e| format!("sign failed: {e}")))
        }) {
            Ok(signed) => {
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
            Err(reason) => {
                // D6: sign failed — surface toast, skip publish. The
                // debug_assert above ensures this arm is unreachable on the
                // guaranteed local-key path (V-111 / #972).
                kernel.set_last_error_toast(Some(reason));
            }
        }
    }

    // ── Publish kind:10002 relay list ─────────────────────────────
    let relay_tags = nip65_tags_from_relay_rows(&relay_rows);
    if let (false, Some(author)) = (relay_tags.is_empty(), identity.active_pubkey()) {
        let unsigned_relay = UnsignedEvent {
            pubkey: author,
            kind: crate::kinds::KIND_RELAY_LIST,
            tags: relay_tags,
            content: String::new(),
            created_at: kernel.now_secs(),
        };
        // Local-key invariant (see kind:0 site above) — synchronous Ready
        // branch via sign_active_nonblocking, no actor stall (D8). V-111 / #972.
        debug_assert!(
            identity.active_remote().is_none(),
            "cold-start kind:10002 sign must run with a local key active (else blocks the actor)"
        );
        match sign_active_nonblocking(identity, &unsigned_relay).and_then(|mut op| {
            op.poll()
                .ok_or_else(|| "sign op pending — remote signer on cold-start path".to_string())
                .and_then(|r| r.map_err(|e| format!("sign failed: {e}")))
        }) {
            Ok(signed) => {
                kernel.prepopulate_author_relay_list(
                    signed.unsigned.pubkey.clone(),
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
            Err(reason) => {
                // D6: sign failed — surface toast, skip publish. The
                // debug_assert above ensures this arm is unreachable on the
                // guaranteed local-key path (V-111 / #972).
                kernel.set_last_error_toast(Some(reason));
            }
        }
    }

    kernel.reconcile_follow_feed_after_identity_change();
    let mut outbound = kernel.active_account_bootstrap_requests();
    outbound.extend(retarget_timeline(identity, kernel, relays_ready));
    outbound.extend(publish_outbound);
    outbound.extend(publish_initial_follows(
        identity,
        kernel,
        &relay_rows,
        initial_follows,
    ));
    outbound
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
fn cold_start_publish_targets(kernel: &Kernel, relay_rows: &[AppRelay]) -> Vec<String> {
    let mut targets: Vec<String> = relay_rows
        .iter()
        .map(|row| row.url.clone())
        .chain(kernel.bootstrap_discovery_relays())
        .collect();
    sort_dedup(&mut targets);
    targets
}

/// Canonicalize the onboarding-declared `(url, role)` pairs into `AppRelay`
/// rows. Returns an empty vec for empty input — there is NO hardcoded default
/// fallback anymore: when the caller declares no relays, the kernel keeps the
/// relay set seeded at `ActorCommand::Start` (or via pre-start
/// `nmp_app_add_relay`). The app, not `nmp-core`, owns the default relay list.
fn relay_rows_from_create_account(relays: &[(String, String)]) -> Vec<AppRelay> {
    relays
        .iter()
        .filter_map(|(url, role)| {
            let url = canonical_relay_url(url)?;
            let raw_role = if role.trim().is_empty() {
                DEFAULT_ONBOARDING_OVERRIDE_ROLE
            } else {
                role
            };
            let role = canonical_relay_role(raw_role).unwrap_or_else(|| "both".to_string());
            Some(AppRelay::new(url, role))
        })
        .collect()
}

fn nip65_tags_from_relay_rows(rows: &[AppRelay]) -> Vec<Vec<String>> {
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

/// Publish the cold-start kind:3 contacts list (the app-supplied
/// `initial_follows`) for a brand-new account.
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
///
/// `follows` is operator policy supplied by the app (NMP no longer hardcodes a
/// default follow set — #1493). An empty list means no contacts to announce, so
/// no kind:3 is signed or published.
fn publish_initial_follows(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    relay_rows: &[AppRelay],
    follows: &[String],
) -> Vec<OutboundMessage> {
    if follows.is_empty() {
        return Vec::new();
    }
    let Some(author) = identity.active_pubkey() else {
        return Vec::new();
    };
    let tags = follows
        .iter()
        .map(|p| vec!["p".to_string(), p.clone()])
        .collect::<Vec<_>>();
    let unsigned = UnsignedEvent {
        pubkey: author,
        kind: 3,
        tags,
        content: String::new(),
        created_at: kernel.now_secs(),
    };
    // Local-key invariant: `publish_initial_follows` is only called from
    // `create_account` (after a fresh local key is activated), so
    // `sign_active_nonblocking` takes the synchronous Ready branch — no remote
    // round-trip, no actor stall (D8). V-111 / #972 removed the blocking
    // primitive entirely.
    debug_assert!(
        identity.active_remote().is_none(),
        "cold-start kind:3 sign must run with a local key active (else blocks the actor)"
    );
    match sign_active_nonblocking(identity, &unsigned).and_then(|mut op| {
        op.poll()
            .ok_or_else(|| "sign op pending — remote signer on cold-start path".to_string())
            .and_then(|r| r.map_err(|e| format!("sign failed: {e}")))
    }) {
        Ok(signed) => {
            let target_relays = cold_start_publish_targets(kernel, relay_rows);
            if target_relays.is_empty() {
                // D6: no usable cold-start relay — surface a toast, never
                // panic. The follow set is already pre-populated locally
                // (`prepopulate_contacts`); the user can re-publish
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
