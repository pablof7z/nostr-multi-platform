//! The actual `window.nostr.signEvent(...)` bridge.
//!
//! Single responsibility: turn an [`UnsignedEvent`] into either a
//! [`SignerOp::Pending(rx)`] (the trait-compatible synchronous-by-default
//! shape `Signer::sign()` returns) OR a real `Future` (the pure-async twin
//! the wasm runtime's Promise wrapper awaits — see
//! [`sign_event_via_extension`]).
//!
//! Split out of `nip07.rs` to keep that file under the AGENTS.md 500-LOC
//! ceiling. Brought in via `#[path = "nip07/wasm.rs"]` so the module path is
//! still `crate::signers::nip07::wasm`, byte-identical with the prior
//! inline-module shape — every existing `pub use nip07::wasm::*` re-export
//! resolves unchanged.
//!
//! ## Flow (mpsc shim)
//!
//! 1. Build a JS object matching the NIP-07 event template shape
//!    (`{kind, content, tags, created_at}` — the extension owns
//!    `pubkey` / `id` / `sig`).
//! 2. Look up `window.nostr.signEvent` and invoke it with the template.
//!    NIP-07 spec: returns a Promise resolving to the fully-formed signed
//!    event JSON.
//! 3. `wasm-bindgen-futures::spawn_local` parks the await on the JS task
//!    queue. When the Promise resolves, deserialize the JSON into our
//!    [`SignedEvent`] shape and `tx.send(Ok(event))`.
//!
//! ## Flow (async twin)
//!
//! Same template build + signEvent lookup; the JS Promise is `await`-ed
//! directly through `JsFuture::from(p).await` inside an `async fn` — no
//! mpsc hop, no `spawn_local` indirection. The wasm runtime's Promise
//! wrapper (`NmpWasmRuntime::dispatch_app_action_async`) chains this into
//! `future_to_promise(...)` so the JS caller gets a Promise resolving to
//! the `WorkerEvent` JSON the host already knows how to decode.
//!
//! ## Error mapping
//!
//! - Missing `window` or `window.nostr` → `SignerError::Backend` (no
//!   extension installed).
//! - Promise rejection (user denied / extension internal error) →
//!   `SignerError::Rejected`.
//! - Pubkey mismatch (extension signed with a different key than the one
//!   the caller cached) → `SignerError::Backend`. This guards against a
//!   user switching extension accounts mid-session.
//! - JSON shape that does not round-trip to [`SignedEvent`] →
//!   `SignerError::Backend`.

use std::sync::mpsc;

use js_sys::{Array, Function, Object, Promise, Reflect};
use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::PublicKey;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};

use super::super::SignerOp;
use super::SignerError;

pub(super) fn sign_with_extension(
    cached_pubkey: PublicKey,
    unsigned: UnsignedEvent,
) -> SignerOp<SignedEvent> {
    let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();

    // Build the JS template synchronously — failures here are immediate
    // and surface through the same channel the async path uses, so the
    // caller's `SignerOp::Pending` polling contract holds either way.
    let template = match build_template(&unsigned) {
        Ok(t) => t,
        Err(error) => {
            let _ = tx.send(Err(error));
            return SignerOp::Pending(rx);
        }
    };

    // Look up `window.nostr.signEvent` once; bail out synchronously if
    // the host has no extension installed.
    let sign_event_fn = match resolve_sign_event_fn() {
        Ok(f) => f,
        Err(error) => {
            let _ = tx.send(Err(error));
            return SignerOp::Pending(rx);
        }
    };

    // Invoke the extension. The return value is a Promise; turn it into
    // a `JsFuture` so `spawn_local` can `await` it.
    let promise: Promise = match sign_event_fn.call1(&JsValue::NULL, &template) {
        Ok(value) => match value.dyn_into::<Promise>() {
            Ok(p) => p,
            Err(other) => {
                let _ = tx.send(Err(SignerError::Backend(format!(
                    "window.nostr.signEvent returned non-Promise: {other:?}"
                ))));
                return SignerOp::Pending(rx);
            }
        },
        Err(error) => {
            let _ = tx.send(Err(SignerError::Backend(format!(
                "window.nostr.signEvent invocation threw: {error:?}"
            ))));
            return SignerOp::Pending(rx);
        }
    };

    // Move the unsigned template + tx + cached_pubkey into the spawned
    // future. `spawn_local` parks the await; the resolved JsValue (the
    // signed event JSON, as a JS object) is mapped back to our typed
    // `SignedEvent`. Send-disconnect (caller dropped the SignerOp before
    // the Promise resolved) is silently swallowed — that's the cancel
    // signal.
    spawn_local(async move {
        let result = match JsFuture::from(promise).await {
            Ok(signed_js) => signed_event_from_js(&signed_js, &unsigned, cached_pubkey),
            Err(error) => Err(SignerError::Rejected(format!(
                "window.nostr.signEvent rejected: {error:?}"
            ))),
        };
        let _ = tx.send(result);
    });

    SignerOp::Pending(rx)
}

/// V-01 Stage 3c — pure-async twin of [`sign_with_extension`].
///
/// Returns a real `Future<Output = Result<SignedEvent, SignerError>>` that
/// the wasm runtime's `dispatch_action_async` Promise wrapper can `.await`
/// directly — without the `SignerOp::Pending(rx)` mpsc hop that the trait
/// `Signer::sign()` returns for native-loop compatibility.
///
/// The mpsc shim cannot be awaited cleanly on wasm: `Receiver::recv_timeout`
/// is blocking (deadlocks the wasm thread per the docstring on
/// `Nip07Signer::sign`) and `try_recv` is busy-poll-only (violates D8 if
/// looped). Both hazards disappear when the JS Promise is awaited inline
/// through `JsFuture` — the `await` yields control to the JS event loop
/// the same way the existing `spawn_local` does, but the resolved value
/// is delivered straight back through the future return chain.
///
/// All validation (template build, pubkey mismatch, JSON shape) is shared
/// with `sign_with_extension` through the same private helpers — there is
/// exactly one source of NIP-07 wire-shape policy in this crate.
///
/// # Errors
///
/// - [`SignerError::Backend`] for missing window / window.nostr / signEvent,
///   non-Promise return, JSON shape mismatch, pubkey mismatch, kind mismatch.
/// - [`SignerError::Rejected`] when the extension rejects the Promise
///   (user denied, internal error).
pub async fn sign_event_via_extension(
    cached_pubkey: PublicKey,
    unsigned: UnsignedEvent,
) -> Result<SignedEvent, SignerError> {
    let template = build_template(&unsigned)?;
    let sign_event_fn = resolve_sign_event_fn()?;
    let promise: Promise = match sign_event_fn.call1(&JsValue::NULL, &template) {
        Ok(value) => value.dyn_into::<Promise>().map_err(|other| {
            SignerError::Backend(format!(
                "window.nostr.signEvent returned non-Promise: {other:?}"
            ))
        })?,
        Err(error) => {
            return Err(SignerError::Backend(format!(
                "window.nostr.signEvent invocation threw: {error:?}"
            )));
        }
    };
    let signed_js = JsFuture::from(promise).await.map_err(|error| {
        SignerError::Rejected(format!("window.nostr.signEvent rejected: {error:?}"))
    })?;
    signed_event_from_js(&signed_js, &unsigned, cached_pubkey)
}

/// Build the JS object the extension expects (`{kind, content, tags,
/// created_at}`). Per NIP-07, `pubkey`/`id`/`sig` are filled in by the
/// extension; supplying them is allowed but optional.
fn build_template(unsigned: &UnsignedEvent) -> Result<JsValue, SignerError> {
    let obj = Object::new();
    Reflect::set(&obj, &JsValue::from_str("kind"), &JsValue::from_f64(f64::from(unsigned.kind)))
        .map_err(|e| {
            SignerError::Backend(format!("failed to set kind on JS template: {e:?}"))
        })?;
    Reflect::set(&obj, &JsValue::from_str("content"), &JsValue::from_str(&unsigned.content))
        .map_err(|e| {
            SignerError::Backend(format!("failed to set content on JS template: {e:?}"))
        })?;
    // f64 precision covers Unix timestamps for ~285M years — safe.
    #[allow(clippy::cast_precision_loss)]
    let created_at = unsigned.created_at as f64;
    Reflect::set(&obj, &JsValue::from_str("created_at"), &JsValue::from_f64(created_at))
        .map_err(|e| {
            SignerError::Backend(format!("failed to set created_at on JS template: {e:?}"))
        })?;
    // Pre-supplying `pubkey` lets the extension cross-check (some
    // implementations error if the template's pubkey disagrees with the
    // active extension account — surfacing a mismatch early is better
    // than signing with the wrong key).
    Reflect::set(
        &obj,
        &JsValue::from_str("pubkey"),
        &JsValue::from_str(&unsigned.pubkey),
    )
    .map_err(|e| {
        SignerError::Backend(format!("failed to set pubkey on JS template: {e:?}"))
    })?;

    // tags is a `string[][]`. Build a JS array of arrays mirroring the
    // Rust `Vec<Vec<String>>` shape verbatim.
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

    Ok(obj.into())
}

/// Resolve `window.nostr.signEvent` or return a structured error if any
/// hop is missing. Hot path is the user has an extension installed.
fn resolve_sign_event_fn() -> Result<Function, SignerError> {
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
    let sign_event = Reflect::get(&nostr, &JsValue::from_str("signEvent")).map_err(|e| {
        SignerError::Backend(format!("window.nostr.signEvent lookup threw: {e:?}"))
    })?;
    sign_event.dyn_into::<Function>().map_err(|other| {
        SignerError::Backend(format!(
            "window.nostr.signEvent is not a function: {other:?}"
        ))
    })
}

/// Convert the extension's resolved JsValue into our typed
/// [`SignedEvent`]. The extension may return a plain JS object — we
/// stringify it via `JSON.stringify` and `serde_json::from_str` so the
/// (loose) NIP-07 shape contract maps onto our own (strict) shape.
///
/// Cross-check: the extension's `pubkey` MUST match the cached pubkey
/// the runtime installed. A mismatch surfaces as `SignerError::Backend`
/// rather than silently producing an event the publish engine would
/// then route under the wrong identity.
fn signed_event_from_js(
    signed_js: &JsValue,
    unsigned: &UnsignedEvent,
    cached_pubkey: PublicKey,
) -> Result<SignedEvent, SignerError> {
    let json = js_sys::JSON::stringify(signed_js)
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

    // NIP-07 returns flat events (`{id, pubkey, kind, tags, content,
    // created_at, sig}`); deserialise into a flat helper first so the
    // pubkey cross-check happens BEFORE we trust any of the other fields.
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

    if flat.pubkey != cached_pubkey.to_hex() {
        return Err(SignerError::Backend(format!(
            "NIP-07 pubkey mismatch: extension signed as {}, cached pubkey was {}; \
             the user may have switched extension accounts — re-handshake required",
            flat.pubkey,
            cached_pubkey.to_hex()
        )));
    }
    // Sanity-check the extension did not silently rewrite the kind or
    // created_at (a few extensions are known to bump created_at to wall
    // clock without warning; that's still a behavioural drift the caller
    // should see, but not a hard error — we honour the extension's
    // values).
    if flat.kind != unsigned.kind {
        return Err(SignerError::Backend(format!(
            "NIP-07 kind mismatch: requested {}, extension produced {}",
            unsigned.kind, flat.kind
        )));
    }

    Ok(SignedEvent {
        id: flat.id,
        sig: flat.sig,
        unsigned: UnsignedEvent {
            pubkey: flat.pubkey,
            kind: flat.kind,
            tags: flat.tags,
            content: flat.content,
            created_at: flat.created_at,
        },
    })
}
