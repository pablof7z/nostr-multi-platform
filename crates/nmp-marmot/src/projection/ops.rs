//! Marmot dispatch + read-projection op handlers.
//!
//! All MDK-input type construction (`GroupId`, `NostrGroupConfigData`) is
//! confined here. No MLS type crosses the projection/action boundary: every op
//! consumes / produces JSON, `group_id` is hex, errors are strings.
//!
//! The per-action write handlers (`publish_key_package` / `create_group` /
//! `invite` / `send` / `leave` / `remove` / `clear_pending`) live in
//! [`group_writes`] — split out purely to stay under the 500-LOC file-size
//! ceiling (AGENTS.md); [`dispatch`] is still the single router.
//!
//! ## Outbound relay seam — CLOSED (publish direction)
//!
//! Every op publishes its relay-bound events INTERNALLY via
//! [`crate::projection::publish`] through the actor/protocol runtime port — no
//! host relay path. Per-kind routing (each an HONEST `PublishRouteClass`
//! claim — see `nmp_core::publish::PublishRouteClass`, D10):
//! kind:445 → `publish_group_pinned` (`GroupHostPin`; group's relay-pinned
//! list, a cache MISS suppresses dispatch, never author-outbox fallback);
//! kind:30443 key-package → `publish_explicit` with `ImportedOrPresigned`
//! (legacy kind:443 retired 2026-05-31); kind:1059 gift-wrap Welcome →
//! `publish_explicit` with `VerifiedPrivateInbox`, routed to the INVITEE's
//! own resolved kind:10050 DM-inbox relays (see
//! [`welcome::resolve_invitee_inboxes`] — the group's relays are NOT used;
//! a Welcome is addressed to the invitee, exactly like a NIP-17 DM). Publish
//! is fire-and-forget (success == "submitted"); the op result's signed event
//! JSON is INFORMATIONAL only.
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

use serde_json::{json, Value};

use crate::projection::action::MarmotAction;
use crate::projection::payload::MarmotMessageRow;
use crate::projection::state::{hex_encode, InnerHandle};

#[path = "ops/group_writes.rs"]
mod group_writes;
#[path = "ops/input.rs"]
mod input;
#[path = "ops/welcome.rs"]
mod welcome;
use group_writes::{clear_pending, create_group, invite, leave, publish_key_package, remove, send};
use input::group_id_from_hex;

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
        Err(e) => {
            // #3057 round-5: a failed mutating op (e.g. create_group/invite that
            // cannot resolve an invitee's kind:10050 DM-inbox, so A's Welcome is
            // never published) MUST surface the SPECIFIC reason in the Marmot
            // snapshot — not vanish into the generic host action-failure toast.
            // Previously this returned the error only to the action ledger, so
            // the Marmot UI (invites view) showed nothing about why the invite
            // silently produced no group + no Welcome on the wire.
            let op = mutating_op_name(action);
            tracing::warn!(
                target: "nmp_marmot::publish",
                op,
                error = %e,
                "marmot mutating op FAILED — surfacing to last_op_error banner"
            );
            h.record_last_op_failure(
                op.to_string(),
                e.clone(),
                correlation_id.unwrap_or_default().to_string(),
                now_secs,
            );
            err(&e)
        }
    }
}

/// Short op label for the snapshot `last_op_error` banner. Mirrors the wire
/// `"op"` tag so a host can key UI copy off it.
fn mutating_op_name(action: &MarmotAction) -> &'static str {
    match action {
        MarmotAction::PublishKeyPackage { .. } => "publish_key_package",
        MarmotAction::CreateGroup { .. } => "create_group",
        MarmotAction::Invite { .. } => "invite",
        MarmotAction::Send { .. } => "send",
        MarmotAction::Leave { .. } => "leave",
        MarmotAction::Remove { .. } => "remove",
        MarmotAction::AcceptWelcome { .. } => "accept_welcome",
        MarmotAction::DeclineWelcome { .. } => "decline_welcome",
        MarmotAction::ClearPending { .. } => "clear_pending",
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

/// INBOUND ingest seam — CLOSED (the shared core).
///
/// Drives a *signed* `nostr::Event` into `MarmotService`: kind:1059
/// gift-wrap → [`ingest_giftwrap`] (triage → `process_welcome` → seed the
/// `group_id→relays` cache + cache the pending-welcome row); kind:445 →
/// `process_message`; kind:30443 → cache the peer KeyPackage. Any other kind
/// is a deliberate **silent skip** (`Ok(None)`).
///
/// The crate-owned [`crate::projection::tap::MarmotIngestParser`] is the
/// caller: the kernel delivers every accepted inbound signed Marmot kind here.
/// The parser discards the returned `Result` — but a genuine Welcome-processing
/// failure is NOT lost: [`ingest_giftwrap`] records it to the snapshot-visible
/// `last_op_error` banner BEFORE returning `Err`, so a dropped invite is
/// user-visible (#3057) even though the tap itself stays a D6 no-op (a
/// poisoned/duplicate/malformed event never panics across the host boundary).
///
/// `Ok(Some(Value))` carries per-kind informational payload for tests and
/// deferred-op retry assertions. The projection mutation (pending-welcome row,
/// relay cache, MDK state, `last_op_error`) is the load-bearing effect — the
/// next `nmp.marmot.snapshot` push projection reflects it.
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
        ingest_giftwrap(h, event, now_secs)
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

/// kind:1059 gift-wrap ingest — the #3057 triage chokepoint.
///
/// kind:1059 is a SHARED envelope: NIP-59 gift-wraps carry Marmot Welcomes
/// (inner kind:444) AND NIP-17 DMs (inner kind:14/15) AND any other protocol's
/// sealed rumor. The Marmot ingest parser is handed EVERY delivered kind:1059
/// addressed to us, so it must distinguish three cases and treat only ONE as a
/// surface-worthy failure:
///
/// 1. **Not ours / not decryptable** — `unwrap_giftwrap` errors (the `#p` tag
///    does not address us, or the NIP-44 decrypt fails). Silent skip
///    (`Ok(None)`): it was never ours to process.
/// 2. **Someone else's protocol** — unwrap succeeds but the inner rumor is NOT
///    a kind:444 `MlsWelcome` (e.g. a NIP-17 DM rumor). Silent skip: the
///    sibling `nip17.dm_inbox` parser owns that envelope; a Marmot "not a
///    welcome" is NOT a Marmot error (surfacing it would spam the banner on
///    every DM — this conflation is exactly the #3057 pre-fix defect).
/// 3. **A Welcome we could not process** — the inner rumor IS a kind:444
///    Welcome, but `process_welcome` fails (e.g. `"No matching key package was
///    found in the key store"`). This is a GENUINE, user-visible failure: a
///    real group invite was delivered and dropped. Pre-#3057-round-2 this
///    returned `Err`, which the tap SILENTLY SWALLOWED — so `pendingWelcomes`
///    stayed empty AND no error surfaced. We now record it to the
///    snapshot-visible `last_op_error` banner (never a silent swallow) and
///    still return `Err` for callers/tests.
fn ingest_giftwrap(
    h: &mut InnerHandle<'_>,
    event: &nostr::Event,
    now_secs: u64,
) -> Result<Option<Value>, String> {
    // Case 1 — unwrap. Not addressed to us / undecryptable → not ours; skip.
    let unwrapped = match h.service().unwrap_giftwrap(event) {
        Ok(u) => u,
        Err(e) => {
            // #3057 instrumentation: a kind:1059 that fails to unwrap is not
            // ours (wrong #p, or NIP-44 decrypt failed with our key). Logged so
            // an on-device retest can distinguish "not ours" from a real drop.
            tracing::debug!(
                target: "nmp_marmot::ingest",
                event_id = %event.id.to_hex(),
                error = %e,
                "marmot giftwrap ingest: unwrap failed (not ours / undecryptable) — skip"
            );
            return Ok(None);
        }
    };
    // Case 2 — only kind:444 MLS Welcome rumors are Marmot's business. Any
    // other gift-wrapped rumor (NIP-17 DM, etc.) belongs to another parser.
    if unwrapped.rumor.kind != nostr::Kind::MlsWelcome {
        tracing::debug!(
            target: "nmp_marmot::ingest",
            event_id = %event.id.to_hex(),
            rumor_kind = unwrapped.rumor.kind.as_u16(),
            "marmot giftwrap ingest: inner rumor is not a kind:444 Welcome — skip (another protocol's envelope)"
        );
        return Ok(None);
    }
    // Case 3 — a real Welcome. Process it; a failure here is surface-worthy.
    let sender = unwrapped.sender;
    tracing::info!(
        target: "nmp_marmot::ingest",
        event_id = %event.id.to_hex(),
        "marmot giftwrap ingest: kind:444 Welcome unwrapped — calling process_welcome"
    );
    match h.service().process_welcome(&event.id, &unwrapped.rumor) {
        Ok(welcome) => {
            let wid = event.id.to_hex();
            let group_name = welcome.group_name.clone();
            // Seed the relay-pinned cache from the Welcome's ground-truth
            // group_relays now, so the eventual post-join self_update kind:445
            // routes correctly even if `accept_welcome`'s re-derive path runs.
            h.cache_group_relays(
                hex_encode(welcome.mls_group_id.as_slice()),
                welcome.group_relays.iter().cloned().collect(),
            );
            h.cache_welcome(wid.clone(), event.clone(), group_name, sender.to_hex());
            tracing::info!(
                target: "nmp_marmot::ingest",
                pending_welcome_id_hex = %wid,
                "marmot giftwrap ingest: Welcome processed → cached as pending"
            );
            Ok(Some(json!({ "kind": 1059, "pending_welcome_id_hex": wid })))
        }
        Err(e) => {
            // #3057: surface the dropped Welcome instead of swallowing it. The
            // gift-wrap id is the correlation handle a host can key the banner
            // + a retry off.
            let reason = e.to_string();
            tracing::warn!(
                target: "nmp_marmot::ingest",
                event_id = %event.id.to_hex(),
                error = %reason,
                "marmot giftwrap ingest: process_welcome FAILED → surfacing welcome_ingest banner"
            );
            h.record_last_op_failure(
                "welcome_ingest".to_string(),
                reason.clone(),
                event.id.to_hex(),
                now_secs,
            );
            Err(reason)
        }
    }
}
