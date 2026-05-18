//! Marmot dispatch + read-projection op handlers.
//!
//! All MDK-input type construction (`GroupId`, `NostrGroupConfigData`) is
//! confined here — the FFI translation-layer exception (see `Cargo.toml`
//! rustdoc). No MLS type crosses the C-ABI: every op consumes / produces
//! JSON, `group_id` is hex, errors are strings.
//!
//! ## Pending-commit discipline (mdk-api.md §7.7)
//!
//! `create_group` / `add_members` / `remove_members` / `self_update`
//! produce an MLS pending commit that MUST be resolved exactly once. This
//! crate publishes via the deferred relay seam (it returns the signed
//! events; Swift relays them), so it cannot observe publish success/failure
//! synchronously. Resolution policy:
//!
//! * The signed `evolution_event` / `welcome_rumors` / gift-wraps are fully
//!   built and returned in the op result — they cannot fail to *produce*.
//! * We `commit()` the pending change eagerly (the events are handed off
//!   intact). If Swift's relay publish later fails, it re-dispatches the
//!   op (idempotent for `send`; for group-state ops a fresh
//!   `self_update`/`invite` re-converges the epoch). This matches the
//!   "do NOT block; return again-needed events" instruction — we never
//!   wedge the group, and `clear` is reachable via the `clear_pending` op.
//! * `leave_group` is SelfRemove: `commit()` is a documented no-op there.

use nostr::{EventBuilder, JsonUtil, Kind, PublicKey, RelayUrl};
use serde_json::{json, Value};

use mdk_core::prelude::{GroupId, NostrGroupConfigData};

use crate::marmot::ffi::err;
use crate::marmot::payload::MarmotMessageRow;
use crate::marmot::state::{hex_encode, parse_signed_event, InnerHandle};

/// Decode a hex MLS group id into a `GroupId`.
fn group_id_from_hex(hex: &str) -> Result<GroupId, String> {
    let bytes = decode_hex(hex).ok_or_else(|| "group_id_hex is not valid hex".to_string())?;
    Ok(GroupId::from_slice(&bytes))
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn str_field<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing or non-string field `{k}`"))
}

fn str_array(v: &Value, k: &str) -> Vec<String> {
    v.get(k)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_pubkeys(npubs: &[String]) -> Result<Vec<PublicKey>, String> {
    npubs
        .iter()
        .map(|s| PublicKey::parse(s).map_err(|e| format!("bad pubkey `{s}`: {e}")))
        .collect()
}

fn parse_relays(urls: &[String]) -> Result<Vec<RelayUrl>, String> {
    urls.iter()
        .map(|s| RelayUrl::parse(s).map_err(|e| format!("bad relay `{s}`: {e}")))
        .collect()
}

/// Pull `signed_key_package_events_json` (array of signed kind:30443/443
/// event JSON strings OR objects) — the KeyPackage-cache seam escape hatch.
fn signed_key_package_events(v: &Value) -> Result<Vec<nostr::Event>, String> {
    let arr = match v.get("signed_key_package_events_json") {
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return Err("signed_key_package_events_json must be an array".into()),
        None => Vec::new(),
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let json = match item {
            Value::String(s) => s,
            other => serde_json::to_string(&other)
                .map_err(|e| format!("re-encode kp event: {e}"))?,
        };
        out.push(parse_signed_event(&json)?);
    }
    Ok(out)
}

/// Newest-N decrypted application messages for one group, newest first.
pub(crate) fn group_messages(
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
    msgs.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then(b.id.cmp(&a.id))
    });
    msgs.into_iter()
        .take(page)
        .map(|m| MarmotMessageRow {
            id: m.id.to_hex(),
            sender_npub: m.pubkey.to_hex(),
            content: m.content.clone(),
            created_at: m.created_at.as_secs(),
            epoch: m.epoch,
        })
        .collect()
}

/// Route + execute one dispatch op envelope.
pub(crate) fn dispatch(h: &mut InnerHandle<'_>, v: &Value, now_secs: u64) -> Value {
    let op = match str_field(v, "op") {
        Ok(o) => o,
        Err(e) => return err(&e),
    };
    let r: Result<Value, String> = match op {
        "publish_key_package" => publish_key_package(h, v, now_secs),
        "create_group" => create_group(h, v),
        "invite" => invite(h, v),
        "send" => send(h, v),
        "leave" => leave(h, v),
        "remove" => remove(h, v),
        "accept_welcome" => accept_welcome(h, v),
        "decline_welcome" => decline_welcome(h, v),
        "ingest_signed_event" => ingest_signed_event(h, v),
        "clear_pending" => clear_pending(h, v),
        other => Err(format!("unknown op `{other}`")),
    };
    match r {
        Ok(mut ok) => {
            if let Value::Object(map) = &mut ok {
                // Handlers may set an explicit `ok:false` for soft-fail
                // envelopes (e.g. the KeyPackage-cache seam). Only inject
                // the success flag when the handler did not decide.
                map.entry("ok").or_insert(Value::Bool(true));
            }
            ok
        }
        Err(e) => err(&e),
    }
}

fn publish_key_package(
    h: &mut InnerHandle<'_>,
    v: &Value,
    now_secs: u64,
) -> Result<Value, String> {
    let relays = parse_relays(&str_array(v, "relays"))?;
    let pubn = h
        .service()
        .publish_key_package(relays)
        .map_err(|e| e.to_string())?;
    h.record_key_package(pubn.d_tag.clone(), now_secs);
    Ok(json!({
        "d_tag": pubn.d_tag,
        // Deferred publish seam: Swift author-write-outbox-publishes both.
        "events": [
            pubn.event_30443.as_json(),
            pubn.event_443.as_json(),
        ],
    }))
}

fn create_group(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let name = str_field(v, "name")?.to_string();
    let description = v
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let relays = parse_relays(&str_array(v, "relays"))?;
    let invitee_npubs = str_array(v, "invitee_npubs");
    let kp_events = signed_key_package_events(v)?;
    if kp_events.is_empty() {
        return Ok(json!({
            "ok": false,
            "error": "key_package_unavailable",
            "needs": invitee_npubs,
            "hint": "supply signed_key_package_events_json (KeyPackage-cache seam)"
        }));
    }
    let admins = vec![h.service().public_key()];
    let config = NostrGroupConfigData::new(
        name,
        description,
        None,
        None,
        None,
        relays,
        admins,
    );
    let (group, pending) = h
        .service()
        .create_group(kp_events, config)
        .map_err(|e| e.to_string())?;
    let group_id_hex = hex_encode(group.mls_group_id.as_slice());
    let welcomes: Vec<String> = pending
        .welcome_rumors
        .iter()
        .map(|r| r.as_json())
        .collect();
    // Deferred publish seam: events handed off intact → commit eagerly so
    // the group is not wedged (see module rustdoc).
    pending.commit().map_err(|e| e.to_string())?;
    Ok(json!({
        "group_id_hex": group_id_hex,
        // kind:444 welcome rumors — Swift NIP-59 gift-wraps + delivers.
        "welcome_rumors": welcomes,
    }))
}

fn invite(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let gid = group_id_from_hex(str_field(v, "group_id_hex")?)?;
    let invitee_npubs = str_array(v, "invitee_npubs");
    let kp_events = signed_key_package_events(v)?;
    if kp_events.is_empty() {
        return Ok(json!({
            "ok": false,
            "error": "key_package_unavailable",
            "needs": invitee_npubs,
            "hint": "supply signed_key_package_events_json (KeyPackage-cache seam)"
        }));
    }
    let pending = h
        .service()
        .add_members(&gid, &kp_events)
        .map_err(|e| e.to_string())?;
    let evolution = pending.evolution_event.as_json();
    let welcomes: Vec<String> = pending
        .welcome_rumors
        .iter()
        .map(|r| r.as_json())
        .collect();
    pending.commit().map_err(|e| e.to_string())?;
    Ok(json!({
        // kind:445 commit → group relay; kind:444 rumors → gift-wrap.
        "evolution_event": evolution,
        "welcome_rumors": welcomes,
    }))
}

fn send(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let gid = group_id_from_hex(str_field(v, "group_id_hex")?)?;
    let text = str_field(v, "text")?.to_string();
    let author = h.service().public_key();
    let rumor = EventBuilder::new(Kind::TextNote, text).build(author);
    let msg = h
        .service()
        .create_message(&gid, rumor)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        // Signed kind:445 (MDK signs with the MLS credential) → group relay.
        "event": msg.as_json(),
        "event_id": msg.id.to_hex(),
    }))
}

fn leave(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let gid = group_id_from_hex(str_field(v, "group_id_hex")?)?;
    let pending = h.service().leave_group(&gid).map_err(|e| e.to_string())?;
    let evolution = pending.evolution_event.as_json();
    // SelfRemove — commit() is a documented no-op (a peer commits it).
    pending.commit().map_err(|e| e.to_string())?;
    Ok(json!({ "evolution_event": evolution }))
}

fn remove(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let gid = group_id_from_hex(str_field(v, "group_id_hex")?)?;
    let pubkeys = parse_pubkeys(&str_array(v, "member_npubs"))?;
    let pending = h
        .service()
        .remove_members(&gid, &pubkeys)
        .map_err(|e| e.to_string())?;
    let evolution = pending.evolution_event.as_json();
    pending.commit().map_err(|e| e.to_string())?;
    Ok(json!({ "evolution_event": evolution }))
}

fn accept_welcome(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let wid = str_field(v, "welcome_id_hex")?.to_string();
    let Some(gift) = h.take_welcome_gift_wrap(&wid) else {
        return Err(format!("no pending welcome `{wid}`"));
    };
    // Idempotent re-derive of the typed Welcome (process_welcome returns
    // the stored one when already processed — verified vs mdk-core 0.8.0).
    let (welcome, _sender) = match h.service().unwrap_and_process_welcome(&gift) {
        Ok(w) => w,
        Err(e) => {
            // Restore so the row reappears for a retry.
            restore(h, &wid, gift);
            return Err(e.to_string());
        }
    };
    if let Err(e) = h.service().accept_welcome(&welcome) {
        restore(h, &wid, gift);
        return Err(e.to_string());
    }
    // MIP-02: post-join self-update is mandatory. Trigger it; return the
    // signed kind:445 commit for Swift to relay-publish.
    let group_id_hex = hex_encode(welcome.mls_group_id.as_slice());
    let self_update = match h.service().self_update(&welcome.mls_group_id) {
        Ok(p) => {
            let ev = p.evolution_event.as_json();
            p.commit().map_err(|e| e.to_string())?;
            Some(ev)
        }
        // Joined OK; the rotation can be retried via the `self_update`
        // path. Don't fail the accept (don't wedge the join).
        Err(_) => None,
    };
    Ok(json!({
        "group_id_hex": group_id_hex,
        "post_join_self_update_event": self_update,
    }))
}

fn decline_welcome(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let wid = str_field(v, "welcome_id_hex")?.to_string();
    let Some(gift) = h.take_welcome_gift_wrap(&wid) else {
        return Err(format!("no pending welcome `{wid}`"));
    };
    let (welcome, _sender) = match h.service().unwrap_and_process_welcome(&gift) {
        Ok(w) => w,
        Err(e) => {
            restore(h, &wid, gift);
            return Err(e.to_string());
        }
    };
    h.service()
        .decline_welcome(&welcome)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "declined": wid }))
}

/// Lossy-observer seam: ingest a *signed* event the kernel observer cannot
/// reconstruct (kind:445 group msg/commit, kind:1059 gift-wrap). Swift
/// passes the full signed event JSON from the relay layer.
fn ingest_signed_event(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let json = str_field(v, "event_json")?;
    let event = parse_signed_event(json)?;
    let kind = event.kind.as_u16();
    if kind == 1059 {
        // Gift-wrap: unwrap + process the inner kind:444 welcome, then
        // cache the gift-wrap as a pending welcome row (no MLS type held).
        match h.service().unwrap_and_process_welcome(&event) {
            Ok((welcome, sender)) => {
                let wid = event.id.to_hex();
                let group_name = welcome.group_name.clone();
                h.cache_welcome(
                    wid.clone(),
                    event,
                    group_name,
                    sender.to_hex(),
                );
                Ok(json!({ "kind": 1059, "pending_welcome_id_hex": wid }))
            }
            Err(e) => Err(e.to_string()),
        }
    } else if kind == 445 {
        // Group message / commit / proposal.
        match h.service().process_message(&event) {
            Ok(_) => Ok(json!({ "kind": 445, "processed": true })),
            Err(e) => Err(e.to_string()),
        }
    } else {
        Err(format!(
            "ingest_signed_event: unsupported kind {kind} (expect 445 or 1059)"
        ))
    }
}

/// Explicit pending-commit clear (mdk-api.md §7.7) — exposed so a caller
/// that detected a relay-publish failure can unwedge the group. Re-runs
/// `self_update` then `clear()`s it (the only sanctioned `MarmotService`
/// path to reach `clear_pending_commit` without a publish).
fn clear_pending(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let gid = group_id_from_hex(str_field(v, "group_id_hex")?)?;
    let pending = h.service().self_update(&gid).map_err(|e| e.to_string())?;
    pending.clear().map_err(|e| e.to_string())?;
    Ok(json!({ "cleared": true }))
}

fn restore(h: &mut InnerHandle<'_>, wid: &str, gift: nostr::Event) {
    // Re-derive display strings best-effort; empty on failure (the row
    // still reappears so the user can retry).
    let (name, npub) = h
        .service()
        .unwrap_and_process_welcome(&gift)
        .map(|(w, s)| (w.group_name.clone(), s.to_hex()))
        .unwrap_or_default();
    h.restore_welcome(wid.to_string(), gift, name, npub);
}
