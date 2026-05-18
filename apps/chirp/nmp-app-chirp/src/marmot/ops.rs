//! Marmot dispatch + read-projection op handlers.
//!
//! All MDK-input type construction (`GroupId`, `NostrGroupConfigData`) is
//! confined here — the FFI translation-layer exception (see `Cargo.toml`
//! rustdoc). No MLS type crosses the C-ABI: every op consumes / produces
//! JSON, `group_id` is hex, errors are strings.
//!
//! ## Outbound relay seam — CLOSED (publish direction)
//!
//! Every op that produces relay-bound events now publishes them
//! INTERNALLY via [`crate::marmot::publish`] (the `nmp-core`
//! `nmp_app_publish_signed_event*` kernel capabilities, called against the
//! retained `*mut NmpApp`) — there is no Swift relay path. Per-kind
//! routing:
//!
//! * **kind:445** (group message / commit / evolution_event / post-join
//!   self-update) → `publish_group_pinned` → the group's configured relay
//!   list (`Explicit`). Marmot groups are relay-pinned. The relay list is
//!   recovered from the `create_group` envelope and
//!   `Welcome::group_relays`; a cache MISS degrades to author-outbox
//!   `Auto` (documented limitation — those events previously did not reach
//!   relays at all, so this is strictly better, not a regression).
//! * **kind:30443 + kind:443** key-package → `publish_author_outbox`
//!   (`Auto` / NIP-65 outbox is correct for key packages). BOTH are
//!   dual-published through 2026-05-31.
//! * **kind:1059** gift-wrap Welcome → the Chirp layer has no NIP-65
//!   inbox-relay resolver for invitees, so these route to the GROUP's
//!   relays as a documented inbox-routing APPROXIMATION (group members
//!   fetch from there). The gift-wrap is already signed with an ephemeral
//!   key (NIP-59) — published verbatim, never re-signed.
//!
//! Publish is fire-and-forget: success == "submitted to the kernel
//! publish pipeline". The op result still carries the signed event JSON
//! but it is now INFORMATIONAL only. The INBOUND ingest seam
//! (`{"op":"ingest_signed_event"}`) is a SEPARATE seam, still open.
//!
//! ## Pending-commit discipline (mdk-api.md §7.7)
//!
//! `create_group` / `add_members` / `remove_members` / `self_update`
//! produce an MLS pending commit that MUST be resolved exactly once.
//! Resolution policy (unchanged — publish is still fire-and-forget so we
//! cannot observe relay success/failure synchronously):
//!
//! * The signed `evolution_event` / `welcome_rumors` / gift-wraps are
//!   fully built, submitted to the kernel publish pipeline, then we
//!   `commit()` the pending change eagerly (the events are produced +
//!   handed to the kernel). If a relay publish later fails, the caller
//!   re-dispatches the op (idempotent for `send`; for group-state ops a
//!   fresh `self_update`/`invite` re-converges the epoch). We never wedge
//!   the group, and `clear` is reachable via the `clear_pending` op.
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
    // kind:30443 + legacy kind:443 → author NIP-65 outbox (Auto). Both
    // dual-published through 2026-05-31 (mdk-api.md §7.4). Internal
    // publish — fire-and-forget via the kernel publish pipeline.
    h.publish_author_outbox(&pubn.event_30443);
    h.publish_author_outbox(&pubn.event_443);
    h.record_key_package(pubn.d_tag.clone(), now_secs);
    Ok(json!({
        "d_tag": pubn.d_tag,
        // INFORMATIONAL only — both events already submitted (Auto outbox).
        "events": [
            pubn.event_30443.as_json(),
            pubn.event_443.as_json(),
        ],
    }))
}

/// NIP-59 gift-wrap each kind:444 welcome rumor for its invitee and
/// publish the resulting signed kind:1059 INTERNALLY.
///
/// Recipient pairing: `welcome_rumors[i]` is paired with
/// `kp_events[i].pubkey` — the key-package event's author IS the invitee
/// MDK built that welcome for. This is the ground truth MDK used (more
/// reliable than the `invitee_npubs` hint, which is caller-supplied and
/// may be reordered / approximate). If the lengths diverge we still wrap
/// every rumor we can pair and skip the rest (best-effort; never panic).
///
/// Inbox-routing APPROXIMATION (documented limitation): the Chirp Rust
/// layer has no NIP-65 inbox-relay resolver for arbitrary invitees, so
/// the kind:1059 is published to the GROUP's relays rather than the
/// recipient's personal inbox relays. Group members fetch welcomes from
/// the group relays, so delivery still converges; a dedicated inbox
/// resolver would tighten this. A group-relay cache miss further degrades
/// to author-outbox `Auto` (kernel empty-relay fallback).
///
/// Returns the signed kind:1059 JSONs (INFORMATIONAL only — already
/// submitted). A `wrap_welcome` failure is surfaced as `Err` (D6 → the
/// op result becomes `{"ok":false,...}`; no panic crosses the FFI).
fn wrap_and_publish_welcomes(
    h: &InnerHandle<'_>,
    group_relays: &[RelayUrl],
    kp_events: &[nostr::Event],
    rumors: &[nostr::UnsignedEvent],
) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(rumors.len());
    for (i, rumor) in rumors.iter().enumerate() {
        // Pair rumor i with key-package i's author (the invitee).
        let Some(kp) = kp_events.get(i) else {
            // More rumors than key-packages should not happen; skip the
            // unpairable tail rather than misroute / panic.
            break;
        };
        let receiver = kp.pubkey;
        let wrapped = h
            .service()
            .wrap_welcome(&receiver, rumor.clone(), None)
            .map_err(|e| e.to_string())?;
        // kind:1059 is ALREADY signed (NIP-59 ephemeral key) — publish
        // verbatim, never re-sign. Inbox approximation → group relays
        // (empty → kernel Auto-fallback).
        h.publish_explicit(&wrapped, group_relays);
        out.push(wrapped.as_json());
    }
    Ok(out)
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
    // group relays (inbox-routing approximation; empty → Auto).
    let welcomes = wrap_and_publish_welcomes(h, &relays, &kp_events, &rumors)?;
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
    let group_id_hex = hex_encode(gid.as_slice());
    // Resolve the relay-pinned relays BEFORE creating the borrowed
    // `pending` (cache read is `&self`; a miss → empty → Auto-fallback).
    let group_relays = h.group_relays(&group_id_hex);
    let pending = h
        .service()
        .add_members(&gid, &kp_events)
        .map_err(|e| e.to_string())?;
    let evolution = pending.evolution_event.as_json();
    // kind:445 commit → group relay-pinned relays (Explicit; cache miss
    // → Auto). MUST go to the group relay(s), not the author outbox.
    h.publish_explicit(&pending.evolution_event, &group_relays);
    let rumors = pending.welcome_rumors.clone();
    // kind:444 rumors → NIP-59 gift-wrap + internal publish.
    let welcomes = wrap_and_publish_welcomes(h, &group_relays, &kp_events, &rumors)?;
    pending.commit().map_err(|e| e.to_string())?;
    Ok(json!({
        // INFORMATIONAL — kind:445 commit + signed kind:1059 gift-wraps,
        // already submitted (group-pinned / inbox-approx routing).
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
    // Signed kind:445 (MDK signs with the MLS credential). Relay-pinned →
    // the group's configured relays (Explicit; cache miss → Auto).
    let group_id_hex = hex_encode(gid.as_slice());
    h.publish_group_pinned(&group_id_hex, &msg);
    Ok(json!({
        // INFORMATIONAL — already submitted to the group-pinned relays.
        "event": msg.as_json(),
        "event_id": msg.id.to_hex(),
    }))
}

fn leave(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let gid = group_id_from_hex(str_field(v, "group_id_hex")?)?;
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

fn remove(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let gid = group_id_from_hex(str_field(v, "group_id_hex")?)?;
    let group_id_hex = hex_encode(gid.as_slice());
    let pubkeys = parse_pubkeys(&str_array(v, "member_npubs"))?;
    let pending = h
        .service()
        .remove_members(&gid, &pubkeys)
        .map_err(|e| e.to_string())?;
    let evolution = pending.evolution_event.as_json();
    // kind:445 remove commit → group relay-pinned relays (Explicit;
    // cache miss → Auto).
    h.publish_group_pinned(&group_id_hex, &pending.evolution_event);
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
    let group_id_hex = hex_encode(welcome.mls_group_id.as_slice());
    // Seed the relay-pinned cache from the GROUND-TRUTH group relays
    // carried in the Welcome (NostrGroupDataExtension → group_relays).
    // MUST happen BEFORE the post-join self_update so that kind:445
    // commit routes to the group relay (Explicit), not the author outbox.
    h.cache_group_relays(
        group_id_hex.clone(),
        welcome.group_relays.iter().cloned().collect(),
    );
    // MIP-02: post-join self-update is mandatory. Trigger it + publish
    // the signed kind:445 commit INTERNALLY to the group-pinned relays.
    let self_update = match h.service().self_update(&welcome.mls_group_id) {
        Ok(p) => {
            let ev = p.evolution_event.as_json();
            h.publish_group_pinned(&group_id_hex, &p.evolution_event);
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
                // Seed the relay-pinned cache from the Welcome's
                // ground-truth group_relays now, so the eventual
                // post-join self_update kind:445 routes correctly even
                // if `accept_welcome`'s re-derive path is taken.
                h.cache_group_relays(
                    hex_encode(welcome.mls_group_id.as_slice()),
                    welcome.group_relays.iter().cloned().collect(),
                );
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
