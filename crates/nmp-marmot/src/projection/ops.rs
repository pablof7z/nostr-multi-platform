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
//! INTERNALLY via [`crate::projection::publish`] (the workspace-internal
//! `nmp_core::NmpApp::publish_signed_explicit` kernel API, called against
//! the retained `&NmpApp`) — there is no Swift relay path. PR-F replaced
//! the prior `extern "C"` block + `unsafe` invocation of
//! `nmp_app_publish_signed_event_to` with this typed Rust call. Per-kind
//! routing:
//!
//! * **kind:445** (group message / commit / evolution_event / post-join
//!   self-update) → `publish_group_pinned` → the group's configured relay
//!   list (`Explicit`). Marmot groups are relay-pinned. The relay list is
//!   recovered from the `create_group` envelope and
//!   `Welcome::group_relays`; a cache MISS degrades to author-outbox
//!   `Auto` (documented limitation — those events previously did not reach
//!   relays at all, so this is strictly better, not a regression).
//! * **kind:30443 + kind:443** key-package → `publish_explicit`
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
//! but it is now INFORMATIONAL only.
//!
//! ## Inbound ingest seam — CLOSED (this is the receive direction)
//!
//! [`ingest_signed_event_core`] is the single code path that drives a
//! signed inbound event into `MarmotService` (kind:1059 →
//! `unwrap_and_process_welcome`; kind:445 → `process_message`; seed the
//! `group_id→relays` cache from `Welcome::group_relays`). It now has TWO
//! callers sharing that one path: the automatic
//! [`crate::projection::tap`] raw-event observer (registered against the
//! retained `*mut NmpApp` in `nmp_marmot_register`; the kernel
//! delivers every accepted inbound signed kind:1059/445 to it) and the
//! back-compat `{"op":"ingest_signed_event"}` dispatch op. The tap makes
//! welcomes / messages received from relays surface in the next
//! `nmp_marmot_snapshot` with no Swift involvement. This was
//! the last open seam.
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
use std::collections::BTreeSet;

use mdk_core::prelude::{GroupId, NostrGroupConfigData};

use crate::projection::display;
use crate::projection::payload::MarmotMessageRow;
use crate::projection::state::{hex_encode, parse_signed_event, InnerHandle};

/// `{"ok":false,"error":"…"}` — local copy of the FFI shell's `err`
/// helper so this layer carries no `crate::marmot::ffi` dependency
/// (Chirp is now a thin C-ABI shell over these modules).
fn err(msg: &str) -> serde_json::Value {
    serde_json::json!({ "ok": false, "error": msg })
}

/// Decode a hex MLS group id into a `GroupId`.
fn group_id_from_hex(hex: &str) -> Result<GroupId, String> {
    let bytes = decode_hex(hex).ok_or_else(|| "group_id_hex is not valid hex".to_string())?;
    Ok(GroupId::from_slice(&bytes))
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
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

/// Resolve the invitee npub list from EITHER the typed array
/// (`invitee_npubs`) OR a free-form text field (`invitee_text`) the UI
/// captures verbatim. Splits on whitespace, comma, semicolon, newline;
/// trims each token; drops empties. Validation (npub/hex parse) stays in
/// the per-op pipeline — this is just the input-adapter step Rust owns
/// per aim.md §4.5 / §6.
fn resolve_invitees(v: &Value) -> Vec<String> {
    let arr = str_array(v, "invitee_npubs");
    if !arr.is_empty() {
        return arr;
    }
    let Some(text) = v.get("invitee_text").and_then(Value::as_str) else {
        return Vec::new();
    };
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
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

/// Resolve the write-relay set for relay-bearing ops.
///
/// The app-wired NIP-65 write relays (`h.write_relay_urls()`, recovered
/// from the live `NmpApp`) are authoritative for the FFI host path. When
/// the projection is driven WITHOUT an app wired (a reusable host that
/// supplies relays directly — e.g. the FFI round-trip tests, or any
/// non-Chirp consumer), fall back to the envelope `relays` array. This
/// keeps `nmp-marmot::projection` host-agnostic: relays come from the
/// kernel when available, otherwise from the caller's op envelope.
fn resolve_write_relays(h: &InnerHandle<'_>, v: &Value) -> Vec<String> {
    let app_relays = h.write_relay_urls();
    if !app_relays.is_empty() {
        return app_relays;
    }
    str_array(v, "relays")
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
            other => {
                serde_json::to_string(&other).map_err(|e| format!("re-encode kp event: {e}"))?
            }
        };
        out.push(parse_signed_event(&json)?);
    }
    Ok(out)
}

fn fill_key_packages_from_cache(
    h: &InnerHandle<'_>,
    invitee_npubs: &[String],
    kp_events: &mut Vec<nostr::Event>,
) -> (Vec<String>, Vec<PublicKey>) {
    let valid_pubkeys = invitee_npubs
        .iter()
        .filter_map(|s| PublicKey::parse(s).ok())
        .collect::<Vec<_>>();
    let cached = h.service().cached_key_packages(&valid_pubkeys);
    let mut present = kp_events
        .iter()
        .map(|event| event.pubkey.to_hex())
        .collect::<BTreeSet<_>>();
    for event in cached {
        if present.insert(event.pubkey.to_hex()) {
            kp_events.push(event);
        }
    }

    let mut needs = Vec::new();
    let mut fetch_pubkeys = Vec::new();
    for invitee in invitee_npubs {
        match PublicKey::parse(invitee) {
            Ok(pk) if present.contains(&pk.to_hex()) => {}
            Ok(pk) => {
                needs.push(invitee.clone());
                fetch_pubkeys.push(pk);
            }
            Err(_) => needs.push(invitee.clone()),
        }
    }
    (needs, fetch_pubkeys)
}

fn missing_key_package_result(
    h: &InnerHandle<'_>,
    needs: Vec<String>,
    fetch_pubkeys: &[PublicKey],
) -> Value {
    let fetch_requested = h.request_key_package_fetch(fetch_pubkeys);
    // Pre-format the abbreviated npub list the UI shows in its error
    // string. Per aim.md §6, formatting belongs in Rust.
    let needs_display: Vec<String> = needs
        .iter()
        .map(|n| display::short_npub_compact(n))
        .collect();
    json!({
        "ok": false,
        "error": "key_package_unavailable",
        "needs": needs,
        "needs_display": needs_display,
        "fetch_requested": fetch_requested,
        "hint": "key package lookup was requested; results arrive via the kernel tap"
    })
}

/// Newest-N decrypted application messages for one group, newest first.
///
/// Preserves the prior wire ordering (DESC) so existing Swift consumers
/// keep working byte-for-byte against an extended row schema.
pub fn group_messages(
    h: &mut InnerHandle<'_>,
    group_id_hex: &str,
    page: usize,
    now_secs: u64,
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
        .map(|m| {
            let sender_npub = m.pubkey.to_hex();
            let sender_short = display::short_npub(&sender_npub);
            // Avatar initials are first 2 ASCII hex chars of the pubkey
            // (preserves the previous Swift derivation, which never
            // matched `hasPrefix("npub1")` against the raw hex string).
            let sender_initials = display::initials(&sender_npub);
            let sender_color_hex = display::avatar_color_hex(&sender_npub);
            let created_at = m.created_at.as_secs();
            let created_at_display = display::relative_time(created_at, now_secs);
            MarmotMessageRow {
                id: m.id.to_hex(),
                sender_npub,
                sender_short,
                sender_initials,
                sender_color_hex,
                content: m.content.clone(),
                created_at,
                created_at_display,
                epoch: m.epoch,
            }
        })
        .collect()
}

/// Route + execute one dispatch op envelope.
pub fn dispatch(h: &mut InnerHandle<'_>, v: &Value, now_secs: u64) -> Value {
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
    _v: &Value,
    now_secs: u64,
) -> Result<Value, String> {
    let urls = resolve_write_relays(h, _v);
    if urls.is_empty() {
        return Err("no write relays configured — add one in Settings > Relays".to_string());
    }
    let relays = parse_relays(&urls)?;
    let pubn = h
        .service()
        .publish_key_package(relays.clone())
        .map_err(|e| e.to_string())?;
    // kind:30443 + legacy kind:443 — both go through the kernel publish
    // pipeline (fire-and-forget, async). The historical synchronous
    // tungstenite "direct EVENT submit" path used to live here as a
    // simulator-path verification fallback but it was a D8 violation: it
    // blocked the calling thread (kernel actor / Swift worker) on
    // synchronous TCP + TLS + per-relay 6 s wall-clock waits. The kernel
    // publish pipeline is the canonical path; no consumer reads the
    // former `direct_ok` / `send_errors` fields (verified across
    // ios/, apps/, crates/).
    use nostr::JsonUtil as _;
    h.publish_explicit(&pubn.event_30443, &relays);
    h.publish_explicit(&pubn.event_443, &relays);
    h.record_key_package(pubn.d_tag.clone(), now_secs);
    Ok(json!({
        "d_tag": pubn.d_tag,
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
/// resolver would tighten this.
///
/// **D10 provenance guard** — a group-relay cache miss (`group_relays` is
/// empty) NO LONGER degrades to author-outbox `Auto`. The
/// [`publish_to`](crate::projection::publish::publish_to) guard refuses to
/// dispatch a kind:1059 envelope without an explicit relay pin (publishing
/// it to the author's public NIP-65 outbox would leak the existence of an
/// MLS Welcome to every public relay the author advertises). The signed
/// kind:1059 JSON still appears in the INFORMATIONAL return so callers
/// have ground-truth audit of what was built; only the wire dispatch is
/// suppressed. To restore delivery in that edge case the group's relays
/// must be cached (`cache_group_relays`) before the welcomes go out.
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
        // (empty → kernel explicit-target fail-closed).
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
    let urls = resolve_write_relays(h, v);
    if urls.is_empty() {
        return Err("no write relays configured — add one in Settings > Relays".to_string());
    }
    let relays = parse_relays(&urls)?;
    let invitee_npubs = resolve_invitees(v);
    let mut kp_events = signed_key_package_events(v)?;
    // Fill from kp_cache (populated by the app's raw-event tap when the
    // kernel delivers peers' kind:30443 events), then require EVERY requested
    // invitee to have a signed KeyPackage. A partial cache must not silently
    // create a group missing some requested members.
    if !invitee_npubs.is_empty() {
        let (needs, fetch_pubkeys) =
            fill_key_packages_from_cache(h, &invitee_npubs, &mut kp_events);
        if !needs.is_empty() {
            return Ok(missing_key_package_result(h, needs, &fetch_pubkeys));
        }
    }
    let admins = vec![h.service().public_key()];
    let config =
        NostrGroupConfigData::new(name, description, None, None, None, relays.clone(), admins);
    let (group, pending) = h
        .service()
        .create_group(kp_events.clone(), config)
        .map_err(|e| e.to_string())?;
    let group_id_hex = hex_encode(group.mls_group_id.as_slice());
    let rumors = pending.welcome_rumors.clone();
    // NIP-59 gift-wrap + internally publish each kind:444 welcome to the
    // group relays (inbox-routing approximation; empty → fail closed).
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
    let invitee_npubs = resolve_invitees(v);
    let mut kp_events = signed_key_package_events(v)?;
    // Fill from kp_cache (populated by the tap), then require EVERY requested
    // invitee to have a signed KeyPackage. A partial cache must not silently
    // invite fewer members than the user requested.
    if !invitee_npubs.is_empty() {
        let (needs, fetch_pubkeys) =
            fill_key_packages_from_cache(h, &invitee_npubs, &mut kp_events);
        if !needs.is_empty() {
            return Ok(missing_key_package_result(h, needs, &fetch_pubkeys));
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
    // the group's configured relays (Explicit; cache miss → fail closed).
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
    // cache miss → fail closed).
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

/// INBOUND ingest seam — CLOSED (the shared core).
///
/// Drives a *signed* `nostr::Event` into `MarmotService`: kind:1059
/// gift-wrap → `unwrap_and_process_welcome` (+ seed the `group_id→relays`
/// cache from `Welcome::group_relays` and cache the pending-welcome row);
/// kind:445 → `process_message`. Any other kind is a deliberate **silent
/// skip** (`Ok(None)`): the raw-event tap registers `[444, 445, 1059]`
/// defensively, and a bare kind:444 rumor (should never reach the wire —
/// the wire welcome is the kind:1059 gift-wrap) must not be treated as an
/// error there.
///
/// TWO callers, ONE path:
///
/// * the automatic [`crate::projection::tap`] raw-event observer (the
///   kernel delivers every accepted inbound signed kind:1059/445 here) —
///   it discards the `Result` (D6: a poisoned/duplicate/malformed event
///   is a silent no-op on the actor thread, never a panic across the
///   FFI), and
/// * the manual `{"op":"ingest_signed_event"}` dispatch op (back-compat /
///   tests) — it maps `Ok(None)` (unsupported kind) and any `Err` to the
///   `{"ok":false,"error":…}` envelope, exactly as before.
///
/// `Ok(Some(Value))` carries the per-kind informational payload the
/// dispatch op echoes. The projection mutation (pending-welcome row,
/// relay cache, MDK state) is the load-bearing effect — the next
/// `nmp_marmot_snapshot` reflects it for BOTH callers.
pub(crate) fn ingest_signed_event_core(
    h: &mut InnerHandle<'_>,
    event: &nostr::Event,
) -> Result<Option<Value>, String> {
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
    } else if kind == 30443 || kind == 443 {
        // KeyPackage: cache the full signed event by author pubkey in the
        // shared MarmotService cache (protocol logic, not Chirp-specific).
        // Any NMP app's tap can call this; create_group/add_members use it
        // as a fallback when the caller supplies no explicit kp_events.
        h.service().cache_key_package(event.clone());
        Ok(Some(
            json!({ "kind": kind, "cached": true, "author": event.pubkey.to_hex() }),
        ))
    } else {
        // Defensive: the tap filter also admits kind:444 (and a bad
        // filter could admit anything). Not an error for the automatic
        // path — a deliberate skip.
        Ok(None)
    }
}

/// Lossy-observer seam back-compat op. Now a thin alias over
/// [`ingest_signed_event_core`] (the raw-event tap is the automatic
/// caller of the same core). Kept so existing tests / any Swift call
/// site that still dispatches `{"op":"ingest_signed_event"}` keep
/// working; an unsupported kind here is still surfaced as an error.
fn ingest_signed_event(h: &mut InnerHandle<'_>, v: &Value) -> Result<Value, String> {
    let json = str_field(v, "event_json")?;
    let event = parse_signed_event(json)?;
    match ingest_signed_event_core(h, &event)? {
        Some(payload) => Ok(payload),
        None => Err(format!(
            "ingest_signed_event: unsupported kind {} (expect 445 or 1059)",
            event.kind.as_u16()
        )),
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
