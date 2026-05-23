//! NIP-07 (browser extension) signer.
//!
//! On non-wasm builds the signer is a structural stub: `pubkey()` returns the
//! cached key the constructor was handed and every other operation returns
//! [`SignerError::Unsupported`]. This keeps the trait shape available
//! everywhere the workspace compiles (Swift FFI integration tests, native
//! conformance harnesses) without pretending wasm-only capabilities exist.
//!
//! On wasm32 + `feature = "wasm"` builds, [`Nip07Signer::sign`] reaches into
//! the JS event loop through `wasm-bindgen-futures::spawn_local` and calls
//! `window.nostr.signEvent(...)`. The returned Promise is awaited off-thread
//! and the resolved signed event is pushed back through an
//! `std::sync::mpsc::Receiver` the [`SignerOp::Pending`] carries to the
//! caller. The caller still drives the op synchronously via `poll()` /
//! `wait()` — this is what lets the actor loop integrate the signer without
//! pulling in tokio (see `nmp-signer-iface::op` for the contract).
//!
//! NIP-04 / NIP-44 namespaces are still `Unsupported` on every build —
//! adding `window.nostr.nip04.*` / `nip44.*` bridges is a follow-up; the
//! Stage 3b scope is event signing only.
//!
//! D6 (no panics across the public surface): `pubkey()` cannot fail because
//! construction is gated on a cached pubkey (see `from_payload`); every
//! other failure mode is a structured [`SignerError`] the caller maps to
//! `toast: Option<String>` at the FFI boundary.

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::PublicKey;

use super::payload::{Nip07Payload, SignerPayload};
use super::traits::{Nip04, Nip44, Signer, SignerBackend, SignerError};
use super::SignerOp;

/// Browser-extension NIP-07 signer.
///
/// ## Construction
///
/// - [`Nip07Signer::from_cached_pubkey`] — restore an extension session with a
///   known pubkey (the common path; the caller has it from a previous payload
///   or from a fresh `window.nostr.getPublicKey()` round-trip).
/// - [`Nip07Signer::from_payload`] — restore from a persisted
///   [`Nip07Payload`].  Returns [`SignerError::NotReady`] if the payload
///   carries no cached pubkey — the caller must re-handshake on wasm before
///   the signer is usable.
///
/// ## Doctrine D6 compliance
///
/// `pubkey()` never panics and never returns `Result` (per trait invariant).
/// Failure modes are surfaced at construction as structured `SignerError`
/// values that callers can map to `toast: Option<String>` at the FFI boundary.
#[derive(Debug)]
pub struct Nip07Signer {
    cached_pubkey: PublicKey,
}

impl Nip07Signer {
    /// Construct from a known cached pubkey.  Always succeeds.
    ///
    /// This is the canonical construction path for both production (after a
    /// wasm `window.nostr.getPublicKey()` round-trip) and restore-from-storage
    /// flows.
    #[must_use]
    pub fn from_cached_pubkey(pubkey: PublicKey) -> Self {
        Self {
            cached_pubkey: pubkey,
        }
    }

    /// Restore from a payload.
    ///
    /// Returns [`SignerError::NotReady`] if the payload carries no cached
    /// pubkey — restore is impossible without a wasm re-handshake.  This is
    /// the structured-error equivalent of the panic this module used to throw
    /// when `pubkey()` was called on an empty signer (D6: errors never cross
    /// FFI as panics).
    pub fn from_payload(p: &Nip07Payload) -> Result<Self, SignerError> {
        let hex = p.cached_pubkey_hex.as_deref().ok_or_else(|| {
            SignerError::NotReady(
                "nip07 payload has no cached pubkey; wasm handshake required \
                 (`window.nostr.getPublicKey()`) before signer is usable"
                    .to_string(),
            )
        })?;
        let pubkey = PublicKey::from_hex(hex).map_err(|e| {
            SignerError::Backend(format!("invalid cached nip07 pubkey hex: {e}"))
        })?;
        Ok(Self {
            cached_pubkey: pubkey,
        })
    }

    /// Whether the current build can actually talk to the extension.
    #[must_use]
    pub const fn nip07_supported() -> bool {
        cfg!(all(target_arch = "wasm32", feature = "wasm"))
    }
}

impl Signer for Nip07Signer {
    fn backend(&self) -> SignerBackend {
        SignerBackend::Nip07
    }

    fn pubkey(&self) -> PublicKey {
        // Construction-gated: `cached_pubkey` is always set.  No panic path.
        self.cached_pubkey
    }

    /// # Wasm hazard: never call `SignerOp::wait()` on the returned value
    ///
    /// On wasm32 the returned `SignerOp::Pending(rx)` only resolves when the
    /// JS event loop runs the `spawn_local` future that awaits
    /// `window.nostr.signEvent(...).then(...)`. `SignerOp::wait()` calls
    /// `recv_timeout`, which blocks the wasm thread — and since wasm32 runs
    /// the JS event loop on the same thread, blocking it prevents the
    /// future from ever resolving. The result: deadlock until the
    /// `recv_timeout` returns `Timeout`.
    ///
    /// On wasm32, callers MUST poll (e.g. yield to JS via another
    /// `spawn_local`, then re-poll). The future-driven publish path Stage 3c
    /// will introduce wraps this hazard inside an `async fn`, so application
    /// code never sees it directly.
    fn sign(&self, unsigned: UnsignedEvent) -> SignerOp<SignedEvent> {
        sign_impl(&self.cached_pubkey, unsigned)
    }

    fn nip04(&self) -> Option<&dyn Nip04> {
        // Extensions may or may not expose `nip04`; we return Some(self) so
        // callers get a clear runtime "Unsupported" rather than guessing.
        Some(self)
    }

    fn nip44(&self) -> Option<&dyn Nip44> {
        Some(self)
    }

    fn to_payload(&self) -> SignerPayload {
        SignerPayload::Nip07(Nip07Payload {
            cached_pubkey_hex: Some(self.cached_pubkey.to_hex()),
        })
    }
}

// ─── sign() backends ─────────────────────────────────────────────────────────
//
// Two compile-time selected implementations:
//
// 1. Non-wasm path (every native target, plus wasm32 builds compiled WITHOUT
//    the `wasm` Cargo feature): no extension to talk to. Returns
//    `Unsupported` synchronously. This is the path the `nmp-testing` fixture
//    and the iOS/macOS/Linux conformance harnesses see.
//
// 2. wasm32 + `feature = "wasm"` path: dispatches to `window.nostr.signEvent`
//    through `wasm-bindgen-futures::spawn_local`. Returns
//    `SignerOp::Pending(rx)` immediately; the actual sign roundtrip completes
//    on the JS event loop, and the caller polls the receiver. No tokio.

#[cfg(not(all(target_arch = "wasm32", feature = "wasm")))]
fn sign_impl(_cached_pubkey: &PublicKey, _unsigned: UnsignedEvent) -> SignerOp<SignedEvent> {
    SignerOp::err(SignerError::Unsupported(
        "NIP-07 signing requires wasm target + browser extension; \
         enable feature = \"wasm\" and target wasm32-unknown-unknown"
            .to_string(),
    ))
}

#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
fn sign_impl(cached_pubkey: &PublicKey, unsigned: UnsignedEvent) -> SignerOp<SignedEvent> {
    wasm::sign_with_extension(*cached_pubkey, unsigned)
}

#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
mod wasm {
    //! The actual `window.nostr.signEvent(...)` bridge.
    //!
    //! Single responsibility: turn an [`UnsignedEvent`] into a
    //! `SignerOp::Pending(rx)` whose receiver eventually carries the signed
    //! event the extension produced.
    //!
    //! ## Flow
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
}

impl Nip04 for Nip07Signer {
    fn encrypt(&self, _recipient: &PublicKey, _plaintext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported(
            "NIP-07 nip04 encrypt: wasm target required".to_string(),
        ))
    }
    fn decrypt(&self, _sender: &PublicKey, _ciphertext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported(
            "NIP-07 nip04 decrypt: wasm target required".to_string(),
        ))
    }
}

impl Nip44 for Nip07Signer {
    fn encrypt(&self, _recipient: &PublicKey, _plaintext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported(
            "NIP-07 nip44 encrypt: wasm target required".to_string(),
        ))
    }
    fn decrypt(&self, _sender: &PublicKey, _payload: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported(
            "NIP-07 nip44 decrypt: wasm target required".to_string(),
        ))
    }
}
