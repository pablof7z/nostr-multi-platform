//! Write-op handlers for [`super::dispatch`] — split out of `ops.rs` to stay
//! under the 500-LOC file-size ceiling (AGENTS.md). Pure extraction: no
//! behavior change from what `ops::dispatch` used to inline directly.
//!
//! See `ops.rs`'s module doc for the outbound-relay-seam / pending-commit
//! discipline that governs every handler here.

use nostr::{EventBuilder, JsonUtil, Kind};
use serde_json::{json, Value};

use mdk_core::prelude::NostrGroupConfigData;

use crate::projection::action::MarmotAction;
use crate::projection::state::{hex_encode, InnerHandle};

use super::input::{
    fill_key_packages_from_cache, group_id_from_hex, parse_pubkeys, parse_relays, resolve_invitees,
    resolve_write_relays, signed_key_package_events,
};
use super::welcome;

pub(super) fn publish_key_package(
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
    // Honest claim: this is a service-signed presigned event submitted
    // outside the normal signer-port publish path (`ImportedOrPresigned`),
    // not a verified-private-inbox or group-relay-pin publish.
    use nostr::JsonUtil as _;
    h.publish_explicit(
        &pubn.event_30443,
        &relays,
        nmp_core::publish::PublishRouteClass::ImportedOrPresigned,
    );
    h.record_key_package(pubn.d_tag.clone(), now_secs);
    Ok(json!({
        "d_tag": pubn.d_tag,
        "events": [
            pubn.event_30443.as_json(),
        ],
    }))
}

pub(super) fn create_group(
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
    // D10 / ordering discipline: resolve EVERY invitee's kind:10050
    // DM-inbox relay set BEFORE any MLS roster mutation. A Marmot Welcome
    // can only honestly claim `VerifiedPrivateInbox` once its recipient's
    // inbox is actually resolved (mirroring `nmp-nip17`'s DM-send gate);
    // failing here — before `service.create_group` touches MDK state —
    // means a failed `create_group` never leaves a phantom member in the
    // local roster and is cleanly retryable.
    let invitee_inboxes = welcome::resolve_invitee_inboxes(h, &kp_events)?;
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
    // invitee's own verified kind:10050 DM-inbox relays (`VerifiedPrivateInbox`).
    let welcomes = welcome::wrap_and_publish_welcomes(h, &kp_events, &rumors, &invitee_inboxes)?;
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

pub(super) fn invite(
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
    // D10 / ordering discipline: resolve EVERY invitee's kind:10050
    // DM-inbox relay set BEFORE `service.add_members` mutates the MLS
    // roster — see the matching comment in `create_group`. A failure here
    // never touches MDK state, so a failed `invite` is cleanly retryable
    // instead of leaving a phantom member (`mdk error: Duplicate signature
    // key in proposals and group` on the next attempt).
    let invitee_inboxes = welcome::resolve_invitee_inboxes(h, &kp_events)?;
    let pending = h
        .service()
        .add_members(&gid, &kp_events)
        .map_err(|e| e.to_string())?;
    let evolution = pending.evolution_event.as_json();
    // kind:445 commit → group relay-pinned relays (`GroupHostPin`; cache
    // miss → fail closed). MUST go to the group relay(s), not the author
    // outbox.
    h.publish_explicit(
        &pending.evolution_event,
        &group_relays,
        nmp_core::publish::PublishRouteClass::GroupHostPin,
    );
    let rumors = pending.welcome_rumors.clone();
    // kind:444 rumors → NIP-59 gift-wrap + internal publish to each
    // invitee's own verified kind:10050 DM-inbox relays (`VerifiedPrivateInbox`).
    let welcomes = welcome::wrap_and_publish_welcomes(h, &kp_events, &rumors, &invitee_inboxes)?;
    pending.commit().map_err(|e| e.to_string())?;
    Ok(json!({
        // INFORMATIONAL — kind:445 commit (group-pinned) + signed kind:1059
        // gift-wraps (invitee's verified DM inbox), already submitted.
        "evolution_event": evolution,
        "welcome_rumors": welcomes,
    }))
}

pub(super) fn send(
    h: &mut InnerHandle<'_>,
    group_id_hex: &str,
    text: &str,
) -> Result<Value, String> {
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

pub(super) fn leave(h: &mut InnerHandle<'_>, group_id_hex: &str) -> Result<Value, String> {
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

pub(super) fn remove(
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

/// Explicit pending-commit clear (mdk-api.md §7.7) — exposed so a caller
/// that detected a relay-publish failure can unwedge the group. Re-runs
/// `self_update` then `clear()`s it (the only sanctioned `MarmotService`
/// path to reach `clear_pending_commit` without a publish).
pub(super) fn clear_pending(h: &mut InnerHandle<'_>, group_id_hex: &str) -> Result<Value, String> {
    let gid = group_id_from_hex(group_id_hex)?;
    let pending = h.service().self_update(&gid).map_err(|e| e.to_string())?;
    pending.clear().map_err(|e| e.to_string())?;
    Ok(json!({ "cleared": true }))
}
