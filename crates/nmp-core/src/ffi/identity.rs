//! T66a identity / publish / multi-account / relay-edit FFI wrappers.
//!
//! Split out of `ffi/mod.rs` to keep both files under the 500-LOC hard cap.
//! These reuse the parent module's validated-argument helpers
//! (`app_ref`, `c_string_argument`, `c_optional_string_argument`) and the
//! shared `NmpApp` handle; the symbols stay `#[no_mangle] extern "C"` so the
//! Swift bridge sees a flat C ABI regardless of the Rust module split.

use super::{app_ref, c_optional_string_argument, c_string_argument, NmpApp};
use crate::actor::ActorCommand;
use crate::kernel::{is_hex_id, is_hex_pubkey};
use std::ffi::c_char;

#[no_mangle]
pub extern "C" fn nmp_app_signin_nsec(app: *mut NmpApp, secret: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    // Wrap the plaintext nsec in `Zeroizing` the instant it is copied out of
    // the C string. The nsec inevitably crosses the FFI boundary as bytes
    // (it MUST be imported somehow); `Zeroizing` does not eliminate that
    // transit, but it guarantees this Rust-side copy is wiped on drop —
    // including the path where `send_cmd` fails and `secret` is dropped here.
    let Some(secret) = c_string_argument(secret).map(zeroize::Zeroizing::new) else {
        return;
    };
    app.send_cmd(ActorCommand::SignInNsec { secret });
}

#[no_mangle]
pub extern "C" fn nmp_app_signin_bunker(app: *mut NmpApp, uri: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };
    app.send_cmd(ActorCommand::SignInBunker { uri });
}

#[no_mangle]
pub extern "C" fn nmp_app_create_new_account(
    app: *mut NmpApp,
    profile_json: *const c_char,
    relays_json: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(profile_json) = c_string_argument(profile_json) else {
        return;
    };
    let Some(relays_json) = c_string_argument(relays_json) else {
        return;
    };

    let profile: std::collections::HashMap<String, String> =
        match serde_json::from_str(&profile_json) {
            Ok(p) => p,
            Err(_) => {
                app.send_cmd(ActorCommand::ShowToast {
                    message: "Failed to decode profile JSON".to_string(),
                });
                return;
            }
        };

    let relays: Vec<(String, String)> = match serde_json::from_str(&relays_json) {
        Ok(r) => r,
        Err(_) => {
            app.send_cmd(ActorCommand::ShowToast {
                message: "Failed to decode relays JSON".to_string(),
            });
            return;
        }
    };

    app.send_cmd(ActorCommand::CreateAccount { profile, relays });
}

#[no_mangle]
pub extern "C" fn nmp_app_switch_active(app: *mut NmpApp, identity_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(identity_id) = c_string_argument(identity_id) else {
        return;
    };
    app.send_cmd(ActorCommand::SwitchActive { identity_id });
}

#[no_mangle]
pub extern "C" fn nmp_app_remove_account(app: *mut NmpApp, identity_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(identity_id) = c_string_argument(identity_id) else {
        return;
    };
    app.send_cmd(ActorCommand::RemoveAccount { identity_id });
}

#[no_mangle]
pub extern "C" fn nmp_app_publish_note(
    app: *mut NmpApp,
    content: *const c_char,
    reply_to_id_or_null: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(content) = c_string_argument(content) else {
        return;
    };
    let reply_to_id = c_optional_string_argument(reply_to_id_or_null);
    app.send_cmd(ActorCommand::PublishNote {
        content,
        reply_to_id,
    });
}

/// Generic publish entrypoint — sign + publish an `UnsignedEvent` already
/// constructed by any protocol-crate builder
/// (`nmp_nip23::Article`, `nmp_nip01::Note`, `nmp_relations::Reaction`, …).
///
/// `unsigned_json` is the JSON serialization of [`crate::substrate::UnsignedEvent`]
/// (fields: `pubkey`, `kind`, `tags`, `content`, `created_at`). The caller's
/// `pubkey` is ignored — signing derives the pubkey from the active identity's
/// keys.
///
/// D6 — malformed JSON is never silently dropped. A [`ActorCommand::ShowToast`]
/// is enqueued so the error surfaces as kernel snapshot state, not a silent
/// no-op. This closes the codex-batch finding from review `e895c09` (Finding 3:
/// FFI silent malformed JSON at `ffi/identity.rs:105`).
#[no_mangle]
pub extern "C" fn nmp_app_publish_unsigned_event(
    app: *mut NmpApp,
    unsigned_json: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(json) = c_string_argument(unsigned_json) else {
        return;
    };
    match serde_json::from_str::<crate::substrate::UnsignedEvent>(&json) {
        Ok(unsigned) => {
            app.send_cmd(ActorCommand::PublishUnsignedEvent(unsigned));
        }
        Err(_) => {
            // D6 — surface the decode failure as a toast (error becomes state,
            // never a silent no-op across FFI). The FFI layer only has a channel
            // sender, so we delegate to the actor via ShowToast.
            app.send_cmd(ActorCommand::ShowToast {
                message: "Failed to decode action payload".to_string(),
            });
        }
    }
}

/// Generic publish entrypoint for an **already-signed** Nostr event — route a
/// fully-formed, externally-signed event verbatim through the kernel's
/// publish pipeline **without re-signing**.
///
/// Sibling to [`nmp_app_publish_unsigned_event`]. The decisive difference:
/// the kernel's signer is **never** consulted. The caller provides a complete
/// Nostr event that was signed elsewhere (an MDK/Marmot group-message signer,
/// a hardware signer, a relayed NIP-46 broker — anything). The kernel
/// verifies the Schnorr signature + event-id hash and, if valid, routes the
/// event verbatim through the same publish planner / NIP-65 outbox resolver /
/// relay-pin path the unsigned sibling uses. Generic capability (D0 — no
/// MLS/Marmot nouns in the kernel); Marmot is merely the first consumer.
///
/// `event_json` is the standard flat NIP-01 event object:
/// ```json
/// {"id":"<64-hex>","pubkey":"<64-hex>","created_at":<u64>,
///  "kind":<u32>,"tags":[["e","…"],…],"content":"…","sig":"<128-hex>"}
/// ```
///
/// **Behavioral asymmetry vs. the unsigned sibling.** The unsigned path
/// requires an active account (it must sign). This path does **not** — the
/// signature already exists and routing keys off the event's *own* `pubkey`
/// (its kind:10002 outbox), not the active account. Publishing with no
/// active account signed in is valid and supported.
///
/// **Return / error contract** (mirrors the unsigned sibling exactly):
/// returns `()`. The publish is fire-and-forget via the actor channel.
/// D6 — no panic ever crosses the FFI boundary:
/// - null app / null `event_json` / non-UTF-8 → silent no-op (matches
///   sibling: `app_ref` / `c_string_argument` guards).
/// - malformed JSON (not a NIP-01 event object) → a `ShowToast`
///   `"Failed to decode signed event payload"` is enqueued (no publish).
/// - structurally-parsed but **invalid signature or id mismatch** → the
///   actor surfaces `"signed event rejected: <reason>"` as a toast
///   (`<reason>` is `"invalid Schnorr signature"`, `"event id mismatch"`,
///   or a serialization error). No outbound frame, no publish-queue entry —
///   the forged/garbled event is dropped, never published.
/// - valid signed event → routed + dispatched to relays verbatim; `id` and
///   `sig` bytes are carried through unchanged.
#[no_mangle]
pub extern "C" fn nmp_app_publish_signed_event(app: *mut NmpApp, event_json: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(json) = c_string_argument(event_json) else {
        return;
    };
    match serde_json::from_str::<crate::store::RawEvent>(&json) {
        Ok(raw) => {
            // Auto target (NIP-65 outbox) — empty `relays`. Back-compat:
            // this symbol's behavior is byte-identical to before the
            // explicit-target variant landed.
            app.send_cmd(ActorCommand::PublishSignedEvent {
                raw,
                relays: Vec::new(),
            });
        }
        Err(_) => {
            // D6 — surface the decode failure as a toast (error becomes state,
            // never a silent no-op across FFI). Signature/id verification
            // happens on the actor side (`commands::publish_signed_event`);
            // here we only guard the JSON-shape decode.
            app.send_cmd(ActorCommand::ShowToast {
                message: "Failed to decode signed event payload".to_string(),
            });
        }
    }
}

/// Explicit-relay-target sibling of [`nmp_app_publish_signed_event`] — route a
/// fully-formed, externally-signed event verbatim to a **specific** relay set
/// (the named D3 opt-out, `PublishTarget::Explicit`) instead of the author's
/// NIP-65 kind:10002 outbox.
///
/// Same verbatim/no-re-sign/no-active-account semantics as the Auto sibling:
/// the kernel's signer is **never** consulted, Schnorr signature + event-id
/// hash are still verified via the same `store::VerifiedEvent::try_from_raw`
/// gate, and `id`/`sig`/`pubkey`/`tags`/`content` are carried through
/// unchanged. The only difference is relay resolution: the verbatim event is
/// dispatched to exactly the relays in `relays_json`, bypassing the outbox
/// resolver. Marmot is the first consumer (kind:445 group messages → the
/// pinned GROUP relay; kind:1059 gift-wraps → recipient inbox relays);
/// kind:30443/443 key-packages keep using the Auto sibling. Generic
/// capability — no MLS/Marmot nouns in the kernel (D0).
///
/// `event_json` is the standard flat NIP-01 event object (identical schema to
/// the Auto sibling):
/// ```json
/// {"id":"<64-hex>","pubkey":"<64-hex>","created_at":<u64>,
///  "kind":<u32>,"tags":[["e","…"],…],"content":"…","sig":"<128-hex>"}
/// ```
///
/// `relays_json` is a **JSON array of relay-URL strings**, e.g.
/// `["wss://relay.example/","wss://other.example/"]`:
/// - **null pointer / non-UTF-8 / empty array `[]`** → behaves **exactly**
///   like [`nmp_app_publish_signed_event`] (`PublishTarget::Auto`, NIP-65
///   outbox). This is the documented Auto-fallback.
/// - **non-empty array of strings** → `PublishTarget::Explicit { relays }`;
///   the verbatim event is dispatched to exactly those relays.
/// - **malformed JSON / not a JSON array / non-string elements** → a
///   `ShowToast` `"Failed to decode signed event relay targets"` is enqueued
///   and **no publish occurs** (mirrors the `event_json` malformed-decode
///   contract — error becomes kernel state, never a silent no-op).
///
/// **Return / error contract** (mirrors the Auto sibling exactly): returns
/// `()`, fire-and-forget via the actor channel. D6 — no panic crosses FFI:
/// - null app / null `event_json` / non-UTF-8 `event_json` → silent no-op.
/// - malformed `event_json` → `ShowToast` `"Failed to decode signed event
///   payload"`, no publish.
/// - malformed `relays_json` (per above) → `ShowToast` `"Failed to decode
///   signed event relay targets"`, no publish.
/// - structurally-parsed but **invalid signature or id mismatch** → the actor
///   surfaces `"signed event rejected: <reason>"` as a toast. No outbound
///   frame, no publish-queue entry — the forged/garbled event is dropped.
/// - valid signed event → routed + dispatched verbatim to the resolved relay
///   set (explicit or, on Auto-fallback, the NIP-65 outbox); `id`/`sig` bytes
///   carried through unchanged.
#[no_mangle]
pub extern "C" fn nmp_app_publish_signed_event_to(
    app: *mut NmpApp,
    event_json: *const c_char,
    relays_json: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(json) = c_string_argument(event_json) else {
        return;
    };
    // null / non-UTF-8 `relays_json` → Auto fallback (empty relay set).
    let relays: Vec<crate::publish::RelayUrl> = match c_string_argument(relays_json) {
        None => Vec::new(),
        Some(raw_relays) => match serde_json::from_str::<Vec<String>>(&raw_relays) {
            // Empty array → Auto fallback. Non-empty → Explicit.
            Ok(list) => list,
            Err(_) => {
                // Malformed / not a JSON string array → toast, no publish.
                app.send_cmd(ActorCommand::ShowToast {
                    message: "Failed to decode signed event relay targets".to_string(),
                });
                return;
            }
        },
    };
    match serde_json::from_str::<crate::store::RawEvent>(&json) {
        Ok(raw) => {
            let _ = app
                .tx
                .send(ActorCommand::PublishSignedEvent { raw, relays });
        }
        Err(_) => {
            app.send_cmd(ActorCommand::ShowToast {
                message: "Failed to decode signed event payload".to_string(),
            });
        }
    }
}

#[no_mangle]
pub extern "C" fn nmp_app_react(
    app: *mut NmpApp,
    target_event_id: *const c_char,
    reaction: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(target_event_id) = c_string_argument(target_event_id) else {
        return;
    };
    if !is_hex_id(&target_event_id) {
        return;
    }
    let reaction = c_optional_string_argument(reaction).unwrap_or_else(|| "+".to_string());
    app.send_cmd(ActorCommand::React {
        target_event_id,
        reaction,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_follow(app: *mut NmpApp, pubkey: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }
    app.send_cmd(ActorCommand::Follow { pubkey });
}

#[no_mangle]
pub extern "C" fn nmp_app_unfollow(app: *mut NmpApp, pubkey: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }
    app.send_cmd(ActorCommand::Unfollow { pubkey });
}

#[no_mangle]
pub extern "C" fn nmp_app_add_relay(
    app: *mut NmpApp,
    url: *const c_char,
    role: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(url) = c_string_argument(url) else {
        return;
    };
    let role = c_optional_string_argument(role).unwrap_or_else(|| "both".to_string());
    app.send_cmd(ActorCommand::AddRelay { url, role });
}

#[no_mangle]
pub extern "C" fn nmp_app_remove_relay(app: *mut NmpApp, url: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(url) = c_string_argument(url) else {
        return;
    };
    app.send_cmd(ActorCommand::RemoveRelay { url });
}

#[no_mangle]
pub extern "C" fn nmp_app_open_timeline(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::OpenTimeline);
}

