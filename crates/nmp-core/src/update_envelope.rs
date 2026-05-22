//! The canonical wire envelope for the single `update_tx` channel.
//!
//! Every frame on the channel is one tagged outer object:
//!
//! ```json
//! {"t":"snapshot","v":<snapshot>}
//! {"t":"panic","v":{"msg":<thread panic message>}}
//! ```
//!
//! so every consumer decodes exactly **one** discriminated type ([`UpdateEnvelope`]).
//!
//! D6 (FFI clean): the tag *is* the discriminant — no exceptions, no key
//! sniffing. D8 (no extra per-event alloc): the snapshot is already a
//! serialized `String`; it is re-attached by **borrowed** [`RawValue`] (no
//! re-parse, no clone of the payload), so wrapping costs a single outer
//! allocation.
//!
//! ## Actor-death contract (D7)
//!
//! The actor thread runs the kernel loop. If it panics or exits, every
//! subsequent `send_cmd` is silently dropped (the command channel closes with
//! no signal to the host). To make that failure observable, the FFI actor
//! supervisor emits exactly one [`UpdateEnvelope::Panic`] frame on the update
//! channel **before** the channel closes. Hosts MUST treat a `Panic` frame as
//! terminal: the kernel is gone and will not recover within this process —
//! surface a fatal error to the user; do not keep sending commands.

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// Schema version of the periodic snapshot payload (the `{"rev":…,"items":…}`
/// object inside an [`UpdateEnvelope::Snapshot`] frame). Bump on **any**
/// breaking change to a snapshot field — a rename, a removal, or a type change.
///
/// Every emitted snapshot carries this value in its `schema_version` field.
///
/// If `schema_version` doesn't match the version the host was compiled
/// against, the host should show an error and refuse to decode further —
/// **do not silently ignore unknown fields**. A renamed or retyped field
/// decodes to wrong/null data with no diagnostic signal otherwise; a loud
/// mismatch is the only safe failure mode.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Carrier for the [`UpdateEnvelope::Panic`] payload — the actor-thread death
/// signal (D7). `msg` is the captured panic message when the runtime could
/// downcast it (`&str` / `String` payloads); a non-string panic payload
/// degrades to a stable placeholder rather than dropping the frame.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PanicFrame {
    /// Thread panic message, best-effort. Always populated — never empty.
    pub msg: String,
}

/// Borrowing **emit-side** envelope. Used only to *serialize* a frame onto the
/// channel; the snapshot half borrows its already-serialized JSON so wrapping
/// never re-parses or clones the (large) payload (D8).
#[derive(Debug, Serialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum WireEnvelope<'a> {
    /// The periodic `Kernel::make_update` snapshot, already serialized.
    Snapshot(&'a RawValue),
    /// Actor-thread death (D7) — the kernel loop panicked or exited.
    Panic(&'a PanicFrame),
}

/// Owning **consumer-side** envelope. This is the single discriminated type
/// every host (and every `nmp-codegen`-projected shell) decodes the channel
/// into. The snapshot interior stays opaque ([`serde_json::Value`]) on
/// purpose: the contract this type models is the *discriminator*, not the
/// ~30-field snapshot's internals (which remain a crate-internal struct).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum UpdateEnvelope {
    /// A full snapshot — the host replaces its rendered state.
    Snapshot(serde_json::Value),
    /// The actor thread died (panicked or exited). Terminal: the kernel is
    /// gone for this process. Hosts MUST surface a fatal error and stop
    /// sending commands — see the actor-death contract above.
    Panic(PanicFrame),
}

/// Wrap an **already-serialized** snapshot JSON string as
/// `{"t":"snapshot","v":…}` without re-parsing it (D8 — one outer alloc).
///
/// D6: if the snapshot string is somehow not valid JSON the frame is dropped
/// (`None`) rather than panicking.
pub fn wrap_snapshot(snapshot_json: String) -> Option<String> {
    let raw = RawValue::from_string(snapshot_json).ok()?;
    serde_json::to_string(&WireEnvelope::Snapshot(&raw)).ok()
}

/// Build the actor-death frame `{"t":"panic","v":{"msg":…}}` (D7).
///
/// `msg` is the best-effort thread panic message. The result decodes cleanly
/// into [`UpdateEnvelope::Panic`] — unlike an ad-hoc `{"t":"panic","m":…}`
/// string, which does NOT match the envelope's `tag`/`content` schema and
/// would fail `UpdateEnvelope` deserialization.
///
/// D6: this is infallible in practice — a `String` always serializes — but a
/// serialization failure degrades to a hand-written constant frame so the
/// host still receives a decodable terminal signal rather than nothing.
pub fn wrap_panic(msg: impl Into<String>) -> String {
    let frame = PanicFrame { msg: msg.into() };
    serde_json::to_string(&WireEnvelope::Panic(&frame))
        .unwrap_or_else(|_| r#"{"t":"panic","v":{"msg":"actor thread died"}}"#.to_string())
}

/// Best-effort message extraction from a [`std::panic::catch_unwind`] error
/// payload (D7). `panic!("…")`, `unwrap`, and `expect` all produce a `String`
/// or `&'static str` payload; anything else (a non-string `panic_any`)
/// degrades to a stable placeholder so the actor-death frame still fires.
///
/// Factored out of the FFI actor supervisor so the downcast logic — the only
/// part of the actor-death path with a branch — is unit-testable without
/// spawning a thread or crossing the C ABI.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "unknown panic in actor thread".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The on-wire tag value MUST be exactly `"snapshot"` (snake_case), never
    /// the Rust variant casing — hosts pin this string.
    #[test]
    fn tag_strings_are_snake_case_lowercase() {
        let snap = wrap_snapshot(r#"{"rev":7,"open_views":2}"#.to_string())
            .expect("snapshot serializes");
        assert!(
            snap.starts_with(r#"{"t":"snapshot","v":"#),
            "snapshot frame must be tagged t=snapshot: {snap}"
        );
    }

    /// Round-trip the **snapshot** shape through the consumer envelope; the
    /// inner JSON survives byte-for-byte (no lossy re-typing).
    #[test]
    fn round_trip_snapshot_shape() {
        let inner = r#"{"rev":42,"open_views":3,"items":[]}"#;
        let wire = wrap_snapshot(inner.to_string()).expect("serialize");
        let decoded: UpdateEnvelope = serde_json::from_str(&wire).expect("decode");
        match decoded {
            UpdateEnvelope::Snapshot(v) => {
                assert_eq!(v["rev"], serde_json::json!(42));
                assert_eq!(v["open_views"], serde_json::json!(3));
                assert_eq!(v["items"], serde_json::json!([]));
            }
            other => panic!("misclassified snapshot frame: {other:?}"),
        }
    }

    /// D7 — the actor-death frame must be tagged `t=panic` and decode into the
    /// envelope's `Panic` variant. This is the contract that makes
    /// actor-thread death visible to the host instead of a silently dropped
    /// command.
    #[test]
    fn panic_frame_is_tagged_and_round_trips() {
        let wire = wrap_panic("actor panicked: boom");
        assert!(
            wire.starts_with(r#"{"t":"panic","v":"#),
            "panic frame must be tagged t=panic: {wire}"
        );
        let decoded: UpdateEnvelope = serde_json::from_str(&wire).expect("decode");
        match decoded {
            UpdateEnvelope::Panic(p) => assert_eq!(p.msg, "actor panicked: boom"),
            other => panic!("expected Panic, got {other:?}"),
        }
    }

    /// The panic frame must survive a substring scan for `"t":"panic"` — the
    /// fast pre-decode check `KernelBridge.swift` performs on every payload.
    /// Changing the wire shape must not break that host-side discriminator.
    #[test]
    fn panic_frame_contains_panic_tag_substring() {
        let wire = wrap_panic("kernel loop died");
        assert!(
            wire.contains(r#""t":"panic""#),
            "host substring check relies on this exact tag: {wire}"
        );
    }

    /// A panic message containing quotes / backslashes must not corrupt the
    /// frame — serde escapes it, and it round-trips byte-for-byte.
    #[test]
    fn panic_frame_escapes_hostile_message() {
        let nasty = r#"weird "quoted" \ panic"#;
        let wire = wrap_panic(nasty);
        let decoded: UpdateEnvelope = serde_json::from_str(&wire).expect("decode");
        match decoded {
            UpdateEnvelope::Panic(p) => assert_eq!(p.msg, nasty),
            other => panic!("expected Panic, got {other:?}"),
        }
    }

    /// The snapshot schema version starts at `1`. A bump is a deliberate,
    /// breaking-change act; this guards against an accidental edit.
    #[test]
    fn snapshot_schema_version_is_one() {
        assert_eq!(SNAPSHOT_SCHEMA_VERSION, 1);
    }

    /// `panic_message` recovers the message from the two payload shapes
    /// `catch_unwind` actually yields for `panic!` / `unwrap` / `expect`.
    #[test]
    fn panic_message_extracts_string_and_str_payloads() {
        let from_string = std::panic::catch_unwind(|| panic!("{}", "owned panic".to_string()))
            .expect_err("must unwind");
        assert_eq!(panic_message(&*from_string), "owned panic");

        let from_str =
            std::panic::catch_unwind(|| panic!("static str panic")).expect_err("must unwind");
        assert_eq!(panic_message(&*from_str), "static str panic");
    }

    /// A non-string panic payload (`panic_any`) degrades to a stable
    /// placeholder — `panic_message` never itself panics (D6).
    #[test]
    fn panic_message_degrades_non_string_payload() {
        let payload =
            std::panic::catch_unwind(|| std::panic::panic_any(42u32)).expect_err("must unwind");
        assert_eq!(panic_message(&*payload), "unknown panic in actor thread");
    }

    /// D7 end-to-end: the exact actor-supervisor sequence — `catch_unwind`
    /// around a panicking closure, then `panic_message` + `wrap_panic`, then
    /// `send` on the update channel — must deliver one decodable `Panic`
    /// frame before the channel closes. This mirrors the closure in
    /// `ffi/mod.rs::nmp_app_new` so a regression in either helper fails here.
    #[test]
    fn actor_death_emits_decodable_panic_frame_on_channel() {
        use std::sync::mpsc;

        let (tx, rx) = mpsc::channel::<String>();
        let supervisor_tx = tx.clone();

        // Stand in for the spawned actor thread: it panics inside the same
        // `catch_unwind` guard the real supervisor uses.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Drop the actor's own sender, mirroring `update_tx` being
            // moved into `run_actor_with_observers`.
            drop(tx);
            panic!("kernel loop exploded");
        }));

        // Supervisor recovery path — identical to `ffi/mod.rs`.
        if let Err(e) = result {
            let msg = panic_message(&*e);
            let frame = wrap_panic(format!("actor thread died: {msg}"));
            let _ = supervisor_tx.send(frame);
        }
        drop(supervisor_tx);

        // The host receives exactly one terminal, decodable panic frame.
        let frame = rx.recv().expect("panic frame must reach the host");
        match serde_json::from_str::<UpdateEnvelope>(&frame).expect("frame decodes") {
            UpdateEnvelope::Panic(p) => {
                assert!(
                    p.msg.contains("actor thread died")
                        && p.msg.contains("kernel loop exploded"),
                    "panic frame must carry diagnostic context: {}",
                    p.msg
                );
            }
            other => panic!("expected Panic frame, got {other:?}"),
        }
        // Channel closes after the single frame — no further frames.
        assert!(rx.recv().is_err(), "channel must close after the panic frame");
    }
}
