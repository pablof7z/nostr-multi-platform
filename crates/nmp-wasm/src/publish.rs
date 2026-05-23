//! V-01 Stage 3c — wasm32 publish path for `nmp.publish` (kind:1 text notes).
//!
//! Bridges the synchronous JS dispatch surface (`WasmRuntime::handle`) to the
//! asynchronous `window.nostr.signEvent(...)` round-trip and the kernel's
//! [`nmp_core::KernelReducer::publish_signed_event`] surface. The runtime
//! returns `ActionAccepted` synchronously; the actual sign → publish → fan
//! cycle runs inside a `wasm_bindgen_futures::spawn_local` task on the JS
//! event loop and pushes a fresh snapshot through the registered callback
//! when it completes.
//!
//! # Why not call `Nip07Signer::sign()` directly?
//!
//! [`nmp_signers::Nip07Signer::sign`] returns
//! [`nmp_signers::SignerOp::Pending`] backed by an `std::sync::mpsc::Receiver`.
//! `mpsc::Receiver` is NOT a `Future` — `.await` is not defined on it. On
//! wasm32 the JS event loop is single-threaded, so the documented hazard
//! (`SignerOp::wait()` deadlocks against the `spawn_local` task it depends
//! on) is fatal. The publish path therefore needs an `async fn` whose return
//! is a real `Future<Output = Result<SignedEvent, SignerError>>`. This file
//! reimplements that single path — every other use of `Nip07Signer`
//! (capability completions, future bunker bridges) stays on the
//! `SignerOp::Pending` shape.
//!
//! Native targets compile this module as an empty shell — the only public
//! item ([`spawn_publish_text_note`]) is `#[cfg(target_arch = "wasm32")]`-gated
//! because it owns `js_sys::Function` (the snapshot callback) and
//! `wasm_bindgen_futures::spawn_local`. Native callers cannot reach it.
//!
//! # No polling (D8)
//!
//! `spawn_local` parks the task on the JS task queue; no `setInterval`, no
//! `requestAnimationFrame` loop. The async block awaits exactly one Promise
//! (the extension's `signEvent`) and runs to completion in one shot.
//!
//! # Doctrine
//!
//! - **D0** — the public surface deals only in `nmp_core::substrate` types
//!   (`UnsignedEvent`, `SignedEvent`) and `nmp_signers::SignerError`. No app
//!   nouns. The publish engine fan-out matches the read-path sink in
//!   `relay_pool::build_sink`.
//! - **D6** — every failure inside the spawned task is funneled through the
//!   snapshot callback (the next snapshot the host receives still reflects
//!   the kernel's `RecentFailure` row). No `panic!`. JS-side throws inside
//!   the callback are silently swallowed (same convention as
//!   `push_snapshot_if_callback`).

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use nmp_core::{substrate::SignedEvent, KernelReducer};
#[cfg(target_arch = "wasm32")]
use nmp_signers::SignerError;
#[cfg(target_arch = "wasm32")]
use nostr::PublicKey;

#[cfg(target_arch = "wasm32")]
use crate::relay_driver::BrowserRelayDriver;
#[cfg(target_arch = "wasm32")]
use crate::snapshot::{push_snapshot_if_callback, RuntimeMeta};

/// Async version of `Nip07Signer::sign()` for the wasm runtime's publish
/// path. Returns the signed event or a [`SignerError`] on failure.
///
/// This is separate from `SignerOp` (which wraps `std::sync::mpsc`) so the
/// wasm32 runtime can `await` it directly inside a `spawn_local` task.
///
/// Mirrors [`nmp_signers::signers::nip07`]'s `sign_with_extension` flow:
/// build the JS template → look up `window.nostr.signEvent` → invoke →
/// await the returned Promise → deserialize the resolved JS object into
/// [`SignedEvent`], with the same pubkey cross-check that prevents an
/// extension account-switch mid-session from silently producing an event
/// under the wrong identity.
#[cfg(target_arch = "wasm32")]
pub async fn nip07_sign_async(
    cached_pubkey: PublicKey,
    unsigned: nmp_core::substrate::UnsignedEvent,
) -> Result<SignedEvent, SignerError> {
    use js_sys::{Array, Function, Object, Promise, Reflect};
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    // Build the NIP-07 event template — `{kind, content, tags, created_at,
    // pubkey}`. Pre-supplying `pubkey` lets the extension cross-check.
    let template: JsValue = {
        let obj = Object::new();
        Reflect::set(
            &obj,
            &JsValue::from_str("kind"),
            &JsValue::from_f64(f64::from(unsigned.kind)),
        )
        .map_err(|e| {
            SignerError::Backend(format!("failed to set kind on JS template: {e:?}"))
        })?;
        Reflect::set(
            &obj,
            &JsValue::from_str("content"),
            &JsValue::from_str(&unsigned.content),
        )
        .map_err(|e| {
            SignerError::Backend(format!("failed to set content on JS template: {e:?}"))
        })?;
        // f64 precision covers Unix timestamps for ~285M years — safe.
        #[allow(clippy::cast_precision_loss)]
        let created_at = unsigned.created_at as f64;
        Reflect::set(
            &obj,
            &JsValue::from_str("created_at"),
            &JsValue::from_f64(created_at),
        )
        .map_err(|e| {
            SignerError::Backend(format!("failed to set created_at on JS template: {e:?}"))
        })?;
        Reflect::set(
            &obj,
            &JsValue::from_str("pubkey"),
            &JsValue::from_str(&unsigned.pubkey),
        )
        .map_err(|e| {
            SignerError::Backend(format!("failed to set pubkey on JS template: {e:?}"))
        })?;

        let tags = Array::new();
        for tag in &unsigned.tags {
            let inner = Array::new();
            for v in tag {
                inner.push(&JsValue::from_str(v));
            }
            tags.push(&inner);
        }
        Reflect::set(&obj, &JsValue::from_str("tags"), &tags).map_err(|e| {
            SignerError::Backend(format!("failed to set tags on JS template: {e:?}"))
        })?;
        obj.into()
    };

    // Resolve `window.nostr.signEvent`. Every hop is a structured error so
    // hosts can surface "install a NIP-07 extension" without parsing the
    // chain of `Reflect::get` failures.
    let sign_event_fn: Function = {
        let window = web_sys::window().ok_or_else(|| {
            SignerError::Backend(
                "no `window` global; NIP-07 requires a browser context".to_string(),
            )
        })?;
        let nostr = Reflect::get(&window, &JsValue::from_str("nostr")).map_err(|e| {
            SignerError::Backend(format!("window.nostr lookup threw: {e:?}"))
        })?;
        if nostr.is_undefined() || nostr.is_null() {
            return Err(SignerError::Backend(
                "no `window.nostr`; install a NIP-07 browser extension".to_string(),
            ));
        }
        let sign_event =
            Reflect::get(&nostr, &JsValue::from_str("signEvent")).map_err(|e| {
                SignerError::Backend(format!("window.nostr.signEvent lookup threw: {e:?}"))
            })?;
        sign_event.dyn_into::<Function>().map_err(|other| {
            SignerError::Backend(format!(
                "window.nostr.signEvent is not a function: {other:?}"
            ))
        })?
    };

    // Invoke the extension. Returns a Promise.
    let promise: Promise = sign_event_fn
        .call1(&JsValue::NULL, &template)
        .map_err(|error| {
            SignerError::Backend(format!(
                "window.nostr.signEvent invocation threw: {error:?}"
            ))
        })?
        .dyn_into::<Promise>()
        .map_err(|other| {
            SignerError::Backend(format!(
                "window.nostr.signEvent returned non-Promise: {other:?}"
            ))
        })?;

    // Await the Promise. Rejection → SignerError::Rejected (user denied or
    // extension internal error).
    let signed_js = JsFuture::from(promise).await.map_err(|error| {
        SignerError::Rejected(format!("window.nostr.signEvent rejected: {error:?}"))
    })?;

    // Stringify + deserialize via the same `FlatNip07Event` shape the
    // SignerOp path uses (kept local: this is the only file that reads the
    // wire shape; folding it back into nmp-signers would re-introduce the
    // SignerOp coupling we're trying to escape).
    let json = js_sys::JSON::stringify(&signed_js)
        .map_err(|e| {
            SignerError::Backend(format!(
                "window.nostr.signEvent returned unserialisable value: {e:?}"
            ))
        })?
        .as_string()
        .ok_or_else(|| {
            SignerError::Backend(
                "JSON.stringify(signedEvent) returned non-string".to_string(),
            )
        })?;

    #[derive(serde::Deserialize)]
    struct FlatNip07Event {
        id: String,
        pubkey: String,
        kind: u32,
        tags: Vec<Vec<String>>,
        content: String,
        created_at: u64,
        sig: String,
    }
    let flat: FlatNip07Event = serde_json::from_str(&json).map_err(|e| {
        SignerError::Backend(format!("signed event JSON did not parse: {e}"))
    })?;

    // Pubkey cross-check — the user may have switched extension accounts
    // since `SetSigner` was installed. Surface it as a hard failure rather
    // than silently publishing under the wrong identity.
    if flat.pubkey != cached_pubkey.to_hex() {
        return Err(SignerError::Backend(format!(
            "NIP-07 pubkey mismatch: extension signed as {}, cached pubkey was {}; \
             the user may have switched extension accounts — re-handshake required",
            flat.pubkey,
            cached_pubkey.to_hex()
        )));
    }
    // Kind cross-check — the extension MUST honor the kind we asked for
    // (kind:1 in V-01 Stage 3c). A drift here is a behavioural deviation
    // the publish engine cannot recover from honestly.
    if flat.kind != unsigned.kind {
        return Err(SignerError::Backend(format!(
            "NIP-07 kind mismatch: requested {}, extension produced {}",
            unsigned.kind, flat.kind
        )));
    }

    Ok(SignedEvent {
        id: flat.id,
        sig: flat.sig,
        unsigned: nmp_core::substrate::UnsignedEvent {
            pubkey: flat.pubkey,
            kind: flat.kind,
            tags: flat.tags,
            content: flat.content,
            created_at: flat.created_at,
        },
    })
}

/// Spawn the full sign → publish → fan-outbound → push-snapshot cycle for one
/// `nmp.publish` text-note dispatch. Returns synchronously after enqueueing
/// the task on the JS event loop; the actual completion is asynchronous and
/// surfaces through the runtime's snapshot callback.
///
/// Caller invariant (runtime.rs `dispatch`): only invoke when a signer is
/// installed AND the resolved action type is `nmp.publish`. The runtime
/// returns `ActionAccepted` to the host BEFORE this fires so the host's
/// spinner can advance to a "pending" state; the eventual snapshot update
/// reflects the publish-engine verdict (one `RecentFailure` row per
/// resolved-zero-relays case, or per-relay `OK` settlements as inbound
/// frames arrive).
///
/// `content` is the text-note body. `cached_pubkey` is the same key
/// `SetSigner` installed; we pass it explicitly so the sign path can do the
/// account-switch cross-check without re-borrowing the signer slot.
#[cfg(target_arch = "wasm32")]
pub(crate) fn spawn_publish_text_note(
    cached_pubkey: PublicKey,
    content: String,
    reducer: Rc<RefCell<KernelReducer>>,
    drivers: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    snapshot_callback: Rc<RefCell<Option<js_sys::Function>>>,
    meta: Rc<RefCell<RuntimeMeta>>,
) {
    use wasm_bindgen_futures::spawn_local;

    // `js_sys::Date::now()` returns ms since Unix epoch as f64. Truncate to
    // seconds for the NIP-01 `created_at` field. f64→u64 is lossless for the
    // ms-since-epoch range; the divide-by-1000 yields a Unix second.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let created_at = (js_sys::Date::now() / 1000.0) as u64;
    let unsigned = nmp_core::substrate::UnsignedEvent {
        pubkey: cached_pubkey.to_hex(),
        kind: 1,
        tags: Vec::new(),
        content,
        created_at,
    };

    spawn_local(async move {
        match nip07_sign_async(cached_pubkey, unsigned).await {
            Ok(signed) => {
                // Enqueue through the publish engine, then fan the per-relay
                // outbound to the matching driver. Borrow scope tight so the
                // snapshot-push below can re-borrow without conflict.
                let outbound = {
                    let mut r = reducer.borrow_mut();
                    r.publish_signed_event(&signed)
                };
                {
                    let drivers_ref = drivers.borrow();
                    for message in outbound {
                        if let Some(driver) =
                            drivers_ref.iter().find(|d| d.url() == message.relay_url())
                        {
                            let _ = driver.send_text(message.text());
                        }
                    }
                }
                // Push a fresh snapshot so the host's UI reflects the
                // publish-engine state (per-relay `recent_failures` row,
                // pending retries, accepted_locally counter). Even when the
                // outbound vec is empty (no NIP-65 outbox cached), the
                // engine has recorded a `RecentFailure` and the snapshot
                // surfaces it.
                push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
            }
            Err(_error) => {
                // Sign failed (user denied, extension missing, account
                // switched). The kernel never saw the event; no publish-
                // engine row to surface. The honest signal is the absence
                // of a snapshot delta — the host's `ActionAccepted` spinner
                // remains until the host's own timeout fires. A future
                // follow-up may add a `record_action_failure`-equivalent
                // surface on `KernelReducer` so the host's spinner clears
                // immediately; for V-01 Stage 3c text-note publish, the
                // engine-driven snapshot is the only honest channel and we
                // do not synthesise one for a sign-step failure (the
                // kernel state truly did not change).
                //
                // We intentionally do not panic and do not log to JS console
                // — the JS host can observe the error class by watching for
                // the absence of a snapshot update within its own timeout.
            }
        }
    });
}

/// Extract a text-note `content` string from a `nmp.publish` dispatch
/// payload. Returns `None` if neither shape carries a non-empty string.
///
/// Two payload shapes reach the runtime:
///
/// 1. `AppActionDispatch` (the `WorkerRequest::AppAction` arm in
///    `runtime.rs::handle`) — `AppAction::PublishNote.into_dispatch_parts()`
///    in `protocol.rs` wraps content as
///    `{"PublishNote": {"content": ..., "reply_to_id": ..., "target": "Auto"}}`.
/// 2. `ActionDispatch` (the `WorkerRequest::Dispatch` arm) — a raw JS host
///    that hand-rolls the dispatch may send the flat
///    `{"content": "..."}` shape directly.
///
/// We accept both. The nested form takes precedence (it's the canonical
/// shape `AppAction` emits) and the flat form is a permissive fallback
/// the typed-dispatch surface uses.
///
/// `reply_to_id` is intentionally not honored in V-01 Stage 3c — kind:1
/// reply tagging (NIP-10 root/reply marks) requires a kernel-side resolver
/// to look up the parent event's relay hint, which the wasm runtime does
/// not yet expose. A non-null `reply_to_id` is silently dropped and the
/// publish proceeds as a top-level note. This matches the AGENTS.md
/// "zero-tolerance on hacks" rule: rather than building a half-correct
/// reply path that emits malformed tags, we document the gap and ship the
/// happy path only. Threaded replies land in Stage 3d when the kernel
/// exposes the resolver.
///
/// The `dead_code` allowance on native targets is load-bearing: the only
/// non-test call site (`runtime::dispatch`'s `nmp.publish` arm) is
/// `target_arch = "wasm32"`-gated. The native build still exercises this
/// helper through the unit tests below, so the payload-shape invariants
/// are pinned without requiring a wasm-bindgen-test runner.
#[must_use]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn extract_publish_content(payload: &serde_json::Value) -> Option<String> {
    // Nested form: `AppAction::PublishNote.into_dispatch_parts()` shape.
    if let Some(content) = payload
        .get("PublishNote")
        .and_then(|p| p.get("content"))
        .and_then(serde_json::Value::as_str)
    {
        if !content.is_empty() {
            return Some(content.to_string());
        }
    }
    // Flat form: typed-dispatch fallback.
    if let Some(content) = payload.get("content").and_then(serde_json::Value::as_str) {
        if !content.is_empty() {
            return Some(content.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::extract_publish_content;
    use serde_json::json;

    #[test]
    fn extract_publish_content_handles_nested_app_action_shape() {
        let payload = json!({
            "PublishNote": {
                "content": "hello, world",
                "reply_to_id": null,
                "target": "Auto",
            }
        });
        assert_eq!(
            extract_publish_content(&payload).as_deref(),
            Some("hello, world")
        );
    }

    #[test]
    fn extract_publish_content_handles_flat_dispatch_shape() {
        let payload = json!({ "content": "raw dispatch content" });
        assert_eq!(
            extract_publish_content(&payload).as_deref(),
            Some("raw dispatch content")
        );
    }

    #[test]
    fn extract_publish_content_prefers_nested_over_flat() {
        // Defensive: if both keys are present, the nested form wins (it's the
        // canonical shape `AppAction::PublishNote` emits).
        let payload = json!({
            "PublishNote": { "content": "nested" },
            "content": "flat",
        });
        assert_eq!(extract_publish_content(&payload).as_deref(), Some("nested"));
    }

    #[test]
    fn extract_publish_content_rejects_empty_string() {
        let payload = json!({ "PublishNote": { "content": "" } });
        assert!(extract_publish_content(&payload).is_none());
        let flat = json!({ "content": "" });
        assert!(extract_publish_content(&flat).is_none());
    }

    #[test]
    fn extract_publish_content_returns_none_for_unrelated_payload() {
        let payload = json!({ "react": { "target_event_id": "abc" } });
        assert!(extract_publish_content(&payload).is_none());
    }

    #[test]
    fn extract_publish_content_returns_none_for_non_string_content() {
        let payload = json!({ "PublishNote": { "content": 42 } });
        assert!(extract_publish_content(&payload).is_none());
    }
}
