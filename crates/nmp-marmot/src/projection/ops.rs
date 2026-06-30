//! Marmot dispatch + read-projection op handlers.
//!
//! All MDK-input type construction (`GroupId`, `NostrGroupConfigData`) is
//! confined here. No MLS type crosses the projection/action boundary: every op
//! consumes / produces JSON, `group_id` is hex, errors are strings.
//!
//! ## Outbound relay seam — CLOSED (publish direction)
//!
//! Every op publishes its relay-bound events INTERNALLY via
//! [`crate::projection::publish`] through the actor/protocol runtime port — no
//! host relay path. Per-kind routing:
//! kind:445 → `publish_group_pinned` (group's relay-pinned list; a cache
//! MISS suppresses dispatch, never author-outbox fallback); kind:30443 key-package
//! → `publish_explicit` (`Auto` / NIP-65 outbox; legacy kind:443 retired
//! 2026-05-31); kind:1059
//! gift-wrap Welcome → the GROUP's relays as a documented inbox-routing
//! APPROXIMATION (published verbatim, NIP-59 ephemeral key, never re-signed).
//! Publish is fire-and-forget (success == "submitted"); the op result's
//! signed event JSON is INFORMATIONAL only.
//!
//! ## Inbound ingest seam — CLOSED (receive direction)
//!
//! [`ingest_signed_event_core`] is the single path driving signed inbound
//! events into `MarmotService` (1059 welcomes, 445 messages, 30443 KPs).
//! Legacy kind:443 is no longer ingested. The
//! [`crate::projection::tap::MarmotIngestParser`] installed by
//! [`crate::install`] is production ingress.
//!
//! ## Pending-commit discipline (mdk-api.md §7.7)
//!
//! `create_group` / `add_members` / `remove_members` / `self_update` produce
//! an MLS pending commit that MUST be resolved exactly once. Since publish is
//! fire-and-forget (no synchronous relay success/failure), we build + submit
//! the signed `evolution_event` / `welcome_rumors` / gift-wraps then `commit()`
//! the pending change EAGERLY. A later relay failure → the caller re-dispatches
//! (idempotent for `send`; a fresh `self_update`/`invite` re-converges the
//! epoch). We never wedge the group; `clear` is reachable via `clear_pending`.
//! * `leave_group` is SelfRemove: `commit()` is a documented no-op there.

use nostr::{EventBuilder, JsonUtil, Kind};
use serde_json::{json, Value};

use mdk_core::prelude::NostrGroupConfigData;

use crate::projection::action::MarmotAction;
use crate::projection::payload::MarmotMessageRow;
use crate::projection::state::{hex_encode, InnerHandle};

#[path = "ops/input.rs"]
mod input;
#[path = "ops/welcome.rs"]
mod welcome;
use input::{
    fill_key_packages_from_cache, group_id_from_hex, parse_pubkeys, parse_relays, resolve_invitees,
    resolve_write_relays, signed_key_package_events,
};

/// `{"ok":false,"error":"…"}` response helper for action/read-projection
/// handlers.
fn err(msg: &str) -> serde_json::Value {
    serde_json::json!({ "ok": false, "error": msg })
}

/// Newest-N decrypted application messages for one group, newest first.
///
/// Preserves the prior wire ordering (DESC) so existing consumers keep working
/// byte-for-byte against an extended row schema.
pub fn group_messages(
    h: &mut InnerHandle<'_>,
    group_id_hex: &str,
    page: usize,
) -> Vec<MarmotMessageRow> {
    let Ok(gid) = group_id_from_hex(group_id_hex) else {
        return Vec::new();
    };
    let Ok(mut msgs) = h.service().get_messages(&gid) else {
        return Vec::new();
    };
    // MDK returns ascending by display order; we want newest-N.
    msgs.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    msgs.into_iter()
        .take(page)
        .map(|m| MarmotMessageRow {
            id: m.id.to_hex(),
            sender_pubkey_hex: m.pubkey.to_hex(),
            content: m.content.clone(),
            created_at: m.created_at.as_secs(),
            epoch: m.epoch,
        })
        .collect()
}

/// Route + execute one typed dispatch op.
///
/// `correlation_id` is the action-registry id. KP-gated ops park and later
/// record terminal verdicts under that same id; REPL/test callers pass `None`.
pub fn dispatch(
    h: &mut InnerHandle<'_>,
    action: &MarmotAction,
    now_secs: u64,
    correlation_id: Option<&str>,
) -> Value {
    let r: Result<Value, String> = match action {
        MarmotAction::PublishKeyPackage { relays } => publish_key_package(h, relays, now_secs),
        MarmotAction::CreateGroup {
            name,
            description,
            invitee_text,
            invitee_npubs,
            signed_key_package_events_json,
            relays,
        } => create_group(
            h,
            action,
            name,
            description,
            invitee_text.as_deref(),
            invitee_npubs.as_deref(),
            signed_key_package_events_json,
            relays,
            now_secs,
            correlation_id,
        ),
        MarmotAction::Invite {
            group_id_hex,
            invitee_text,
            invitee_npubs,
            signed_key_package_events_json,
        } => invite(
            h,
            action,
            group_id_hex,
            invitee_text.as_deref(),
            invitee_npubs.as_deref(),
            signed_key_package_events_json,
            now_secs,
            correlation_id,
        ),
        MarmotAction::Send { group_id_hex, text } => send(h, group_id_hex, text),
        MarmotAction::Leave { group_id_hex } => leave(h, group_id_hex),
        MarmotAction::Remove {
            group_id_hex,
            member_npubs,
        } => remove(h, group_id_hex, member_npubs),
        MarmotAction::AcceptWelcome { welcome_id_hex } => {
            welcome::accept_welcome(h, welcome_id_hex)
        }
        MarmotAction::DeclineWelcome { welcome_id_hex } => {
            welcome::decline_welcome(h, welcome_id_hex)
        }
        MarmotAction::ClearPending { group_id_hex } => clear_pending(h, group_id_hex),
    };
    match r {
        Ok(mut ok) => {
            if let Value::Object(map) = &mut ok {
                // Pending envelopes must not get `ok`; the typed protocol
                // command leaves `"pending":true` actions in Requested until
                // deferred completion records the terminal verdict.
                let is_pending = map.get("pending").and_then(Value::as_bool).unwrap_or(false);
                if !is_pending {
                    map.entry("ok").or_insert(Value::Bool(true));
                }
                // A genuinely successful op clears any stale error banner.
                if !is_pending && map.get("ok").and_then(Value::as_bool) == Some(true) {
                    h.clear_last_op_error();
                }
            }
            ok
        }
        Err(e) => err(&e),
    }
}

#[cfg(test)]
pub(crate) fn dispatch_json_for_tests(
    h: &mut InnerHandle<'_>,
    v: Value,
    now_secs: u64,
    correlation_id: Option<&str>,
) -> Value {
    match serde_json::from_value::<MarmotAction>(v) {
        Ok(action) => dispatch(h, &action, now_secs, correlation_id),
        Err(e) => err(&format!("invalid MarmotAction: {e}")),
    }
}

fn publish_key_package(
    h: &mut InnerHandle<'_>,
    relays: &[String],
    now_secs: u64,
) -> Result<Value, String> {
    let urls = resolve_write_relays(h, relays);
    if urls.is_empty() {
        return Err("no write relays configured — add one in Settings > Relays".to_string());
    }
    let relays = parse_relays(&urls)?;
    let pubn = h
        .service()
        .publish_key_package(relays.clone())
        .map_err(|e| e.to_string())?;
    // kind:30443 only; legacy kind:443 and the synchronous direct-submit
    // fallback were retired. The kernel publish pipeline is canonical.
    use nostr::JsonUtil as _;
    h.publish_explicit(&pubn.event_30443, &relays);
    h.record_key_package(pubn.d_tag.clone(), now_secs);
    Ok(json!({
        "d_tag": pubn.d_tag,
        "events": [
            pubn.event_30443.as_json(),
        ],
    }))
}

fn create_group(
    h: &mut InnerHandle<'_>,
    action: &MarmotAction,
    name: &str,
    description: &str,
    invitee_text: Option<&str>,
    invitee_npubs: Option<&[String]>,
    signed_key_package_events_json: &[Value],
    relays: &[String],
    now_secs: u64,
    correlation_id: Option<&str>,
) -> Result<Value, String> {
    let urls = resolve_write_relays(h, relays);
    if urls.is_empty() {
        return Err("no write relays configured — add one in Settings > Relays".to_string());
    }
    let relays = parse_relays(&urls)?;
    let invitee_npubs = resolve_invitees(invitee_text, invitee_npubs);
    let mut kp_events = signed_key_package_events(signed_key_package_events_json)?;
    // Fill from kp_cache (populated by Marmot's ingest parser when the
    // kernel delivers peers' kind:30443 events), then require EVERY requested
    // invitee to have a signed KeyPackage. A partial cache must not silently
    // create a group missing some requested members.
    if !invitee_npubs.is_empty() {
        let (needs, fetch_pubkeys) =
            fill_key_packages_from_cache(h, &invitee_npubs, &mut kp_events);
        if !needs.is_empty() {
            return Ok(h.park_or_report_kp_unavailable(
                action,
                "create_group",
                needs,
                &fetch_pubkeys,
                correlation_id,
                now_secs,
            ));
        }
    }
    let admins = vec![h.service().public_key()];
    let config = NostrGroupConfigData::new(
        name.to_string(),
        description.to_string(),
        None,
        None,
        None,
        relays.clone(),
        admins,
    );
    let (group, pending) = h
        .service()
        .create_group(kp_events.clone(), config)
        .map_err(|e| e.to_string())?;
    let group_id_hex = hex_encode(group.mls_group_id.as_slice());
    let rumors = pending.welcome_rumors.clone();
    // NIP-59 gift-wrap + internally publish each kind:444 welcome to the
    // group relays (inbox-routing approximation; empty → fail closed).
    let welcomes = welcome::wrap_and_publish_welcomes(h, &relays, &kp_events, &rumors)?;
    // Events produced + submitted → commit eagerly so the group is not
    // wedged (pending-commit discipline, see module rustdoc). This drops
    // `pending`'s borrow of `h`, so the cache write below is free.
    pending.commit().map_err(|e| e.to_string())?;
    // Seed the relay-pinned cache from the envelope `relays` so this
    // group's later kind:445 sends/commits route to the group relays.
    h.cache_group_relays(group_id_hex.clone(), relays);
    Ok(json!({
        "group_id_hex": group_id_hex,
        // INFORMATIONAL — signed kind:1059 gift-wraps, already submitted.
        "welcome_rumors": welcomes,
    }))
}

fn invite(
    h: &mut InnerHandle<'_>,
    action: &MarmotAction,
    group_id_hex: &str,
    invitee_text: Option<&str>,
    invitee_npubs: Option<&[String]>,
    signed_key_package_events_json: &[Value],
    now_secs: u64,
    correlation_id: Option<&str>,
) -> Result<Value, String> {
    let gid = group_id_from_hex(group_id_hex)?;
    let invitee_npubs = resolve_invitees(invitee_text, invitee_npubs);
    let mut kp_events = signed_key_package_events(signed_key_package_events_json)?;
    // Fill from kp_cache (populated by the ingest parser), then require EVERY requested
    // invitee to have a signed KeyPackage. A partial cache must not silently
    // invite fewer members than the user requested.
    if !invitee_npubs.is_empty() {
        let (needs, fetch_pubkeys) =
            fill_key_packages_from_cache(h, &invitee_npubs, &mut kp_events);
        if !needs.is_empty() {
            return Ok(h.park_or_report_kp_unavailable(
                action,
                "invite",
                needs,
                &fetch_pubkeys,
                correlation_id,
                now_secs,
            ));
        }
    }
    let group_id_hex = hex_encode(gid.as_slice());
    // Resolve the relay-pinned relays BEFORE creating the borrowed
    // `pending` (cache read is `&self`; a miss → explicit target fails closed).
    let group_relays = h.group_relays(&group_id_hex);
    let pending = h
        .service()
        .add_members(&gid, &kp_events)
        .map_err(|e| e.to_string())?;
    let evolution = pending.evolution_event.as_json();
    // kind:445 commit → group relay-pinned relays (Explicit; cache miss
    // → fail closed). MUST go to the group relay(s), not the author outbox.
    h.publish_explicit(&pending.evolution_event, &group_relays);
    let rumors = pending.welcome_rumors.clone();
    // kind:444 rumors → NIP-59 gift-wrap + internal publish.
    let welcomes = welcome::wrap_and_publish_welcomes(h, &group_relays, &kp_events, &rumors)?;
    pending.commit().map_err(|e| e.to_string())?;
    Ok(json!({
        // INFORMATIONAL — kind:445 commit + signed kind:1059 gift-wraps,
        // already submitted (group-pinned / inbox-approx routing).
        "evolution_event": evolution,
        "welcome_rumors": welcomes,
    }))
}

fn send(h: &mut InnerHandle<'_>, group_id_hex: &str, text: &str) -> Result<Value, String> {
    let gid = group_id_from_hex(group_id_hex)?;
    let author = h.service().public_key();
    let rumor = EventBuilder::new(Kind::TextNote, text.to_string()).build(author);
    let msg = h
        .service()
        .create_message(&gid, rumor)
        .map_err(|e| e.to_string())?;
    // Signed kind:445 (MDK signs with the MLS credential). Relay-pinned →
    // the group's configured relays (Explicit; cache miss → fail closed).
    let group_id_hex = hex_encode(gid.as_slice());
    h.publish_group_pinned(&group_id_hex, &msg);
    Ok(json!({
        // INFORMATIONAL — already submitted to the group-pinned relays.
        "event": msg.as_json(),
        "event_id": msg.id.to_hex(),
    }))
}

fn leave(h: &mut InnerHandle<'_>, group_id_hex: &str) -> Result<Value, String> {
    let gid = group_id_from_hex(group_id_hex)?;
    let group_id_hex = hex_encode(gid.as_slice());
    let pending = h.service().leave_group(&gid).map_err(|e| e.to_string())?;
    let evolution = pending.evolution_event.as_json();
    // kind:445 SelfRemove commit → group relay-pinned relays (a peer
    // commits the epoch, but the proposal still ships to the group relay).
    h.publish_group_pinned(&group_id_hex, &pending.evolution_event);
    // SelfRemove — commit() is a documented no-op (a peer commits it).
    pending.commit().map_err(|e| e.to_string())?;
    Ok(json!({ "evolution_event": evolution }))
}

fn remove(
    h: &mut InnerHandle<'_>,
    group_id_hex: &str,
    member_npubs: &[String],
) -> Result<Value, String> {
    let gid = group_id_from_hex(group_id_hex)?;
    let group_id_hex = hex_encode(gid.as_slice());
    let pubkeys = parse_pubkeys(member_npubs)?;
    let pending = h
        .service()
        .remove_members(&gid, &pubkeys)
        .map_err(|e| e.to_string())?;
    let evolution = pending.evolution_event.as_json();
    // kind:445 remove commit → group relay-pinned relays (Explicit;
    // cache miss → fail closed).
    h.publish_group_pinned(&group_id_hex, &pending.evolution_event);
    pending.commit().map_err(|e| e.to_string())?;
    Ok(json!({ "evolution_event": evolution }))
}

/// INBOUND ingest seam — CLOSED (the shared core).
///
/// Drives a *signed* `nostr::Event` into `MarmotService`: kind:1059
/// gift-wrap → `unwrap_and_process_welcome` (+ seed the `group_id→relays`
/// cache from `Welcome::group_relays` and cache the pending-welcome row);
/// kind:445 → `process_message`. Any other kind is a deliberate **silent
/// skip** (`Ok(None)`): the Marmot ingest parser registers the Marmot
/// envelope kinds defensively, and a bare kind:444 rumor (should never reach the wire —
/// the wire welcome is the kind:1059 gift-wrap) must not be treated as an
/// error there.
///
/// The crate-owned [`crate::projection::tap::MarmotIngestParser`] is the
/// caller: the kernel delivers every accepted inbound signed Marmot kind here.
/// The parser discards the `Result` (D6: a poisoned/duplicate/malformed event
/// is a silent no-op on the actor thread, never a panic across a host boundary).
///
/// `Ok(Some(Value))` carries per-kind informational payload for tests and
/// deferred-op retry assertions. The projection mutation (pending-welcome row,
/// relay cache, MDK state) is the load-bearing effect — the next
/// `nmp.marmot.snapshot` push projection reflects it.
pub(crate) fn ingest_signed_event_core(
    h: &mut InnerHandle<'_>,
    event: &nostr::Event,
    now_secs: u64,
) -> Result<Option<Value>, String> {
    event
        .verify()
        .map_err(|e| format!("ingest_signed_event_core: event verification failed: {e}"))?;
    let kind = event.kind.as_u16();
    if kind == 1059 {
        // Gift-wrap: unwrap + process the inner kind:444 welcome, then
        // cache the gift-wrap as a pending welcome row (no MLS type held).
        match h.service().unwrap_and_process_welcome(event) {
            Ok((welcome, sender)) => {
                let wid = event.id.to_hex();
                let group_name = welcome.group_name.clone();
                // Seed the relay-pinned cache from the Welcome's
                // ground-truth group_relays now, so the eventual
                // post-join self_update kind:445 routes correctly even
                // if `accept_welcome`'s re-derive path is taken.
                h.cache_group_relays(
                    hex_encode(welcome.mls_group_id.as_slice()),
                    welcome.group_relays.iter().cloned().collect(),
                );
                h.cache_welcome(wid.clone(), event.clone(), group_name, sender.to_hex());
                Ok(Some(json!({ "kind": 1059, "pending_welcome_id_hex": wid })))
            }
            Err(e) => Err(e.to_string()),
        }
    } else if kind == 445 {
        // Group message / commit / proposal.
        match h.service().process_message(event) {
            Ok(_) => Ok(Some(json!({ "kind": 445, "processed": true }))),
            Err(e) => Err(e.to_string()),
        }
    } else if kind == 30443 {
        // kind:30443 KeyPackage: cache the full signed event by author pubkey in
        // the shared MarmotService cache (protocol logic, not Chirp-specific).
        // Any NMP app's tap can call this; create_group/add_members use it as
        // a fallback when the caller supplies no explicit kp_events.
        // (Legacy kind:443 was retired 2026-05-31 and is no longer ingested.)
        let pubkey_hex = event.pubkey.to_hex();
        h.service().cache_key_package(event.clone());
        // After caching, re-run any pending ops unblocked by this KP and age
        // out expired ones (D8 wall-clock gate; `now_secs` is caller-supplied
        // so tests can use synthetic time). The retry + terminal-verdict
        // bookkeeping lives in the `deferred` module.
        h.retry_unblocked_ops(&pubkey_hex, now_secs);
        Ok(Some(
            json!({ "kind": kind, "cached": true, "author": pubkey_hex }),
        ))
    } else {
        // Defensive: the tap filter also admits kind:444 (and a bad
        // filter could admit anything). Not an error for the automatic
        // path — a deliberate skip.
        Ok(None)
    }
}

/// Explicit pending-commit clear (mdk-api.md §7.7) — exposed so a caller
/// that detected a relay-publish failure can unwedge the group. Re-runs
/// `self_update` then `clear()`s it (the only sanctioned `MarmotService`
/// path to reach `clear_pending_commit` without a publish).
fn clear_pending(h: &mut InnerHandle<'_>, group_id_hex: &str) -> Result<Value, String> {
    let gid = group_id_from_hex(group_id_hex)?;
    let pending = h.service().self_update(&gid).map_err(|e| e.to_string())?;
    pending.clear().map_err(|e| e.to_string())?;
    Ok(json!({ "cleared": true }))
}
