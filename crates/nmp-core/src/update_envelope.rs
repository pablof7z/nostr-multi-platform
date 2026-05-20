//! The canonical wire envelope for the single `update_tx` channel.
//!
//! The actor pushes **two structurally distinct** JSON shapes onto the one
//! `Sender<String>` update channel:
//!
//! 1. a **discrete** [`KernelUpdate`] enum — e.g. `{"ViewOpened":{…}}` /
//!    `{"UriRejected":{…}}` — emitted from the `ActorCommand::Kernel` arm; and
//! 2. the **periodic snapshot** produced by `Kernel::make_update` — the large
//!    `{"rev":…,"items":[…],"metrics":{…},…}` object every host renders.
//!
//! Without a discriminator every consumer (Pulse, future Android/desktop
//! shells, `nmp-codegen`-projected host enums) would have to *guess* which
//! shape arrived by sniffing keys. That is undocumented and unsafe.
//!
//! This module makes the contract explicit and singular: **every** frame on
//! the channel is wrapped in one tagged outer object —
//!
//! ```json
//! {"t":"update","v":<KernelUpdate>}
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

use crate::app::KernelUpdate;

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

/// Schema version of the **discrete** [`KernelUpdate`] delta payload (the
/// `{"t":"update","v":…}` arm). The snapshot has carried
/// [`SNAPSHOT_SCHEMA_VERSION`] since PR #25, but the discrete delta variants
/// had **no** version: adding, renaming, or retyping a `KernelUpdate` variant
/// was a silently breaking change on the host with no diagnostic signal.
///
/// Every emitted delta frame now carries this value in a `schema_version`
/// field adjacent to the flattened `KernelUpdate` payload. Bump it on **any**
/// breaking change to a discrete-update variant — a rename, a removal, or a
/// payload-field type change.
///
/// The consumer-side field is `#[serde(default)]`: a frame produced by an
/// older kernel that predates this constant still decodes cleanly, with
/// `schema_version` defaulting to `1`. A host that does not yet inspect the
/// field is therefore unaffected.
pub const DELTA_SCHEMA_VERSION: u8 = 1;

/// `serde` default for the consumer-side delta `schema_version` — keeps a
/// pre-versioning frame (no `schema_version` key) decodable (defaults to `1`).
fn default_delta_schema_version() -> u8 {
    DELTA_SCHEMA_VERSION
}

/// Carrier for the [`UpdateEnvelope::Panic`] payload — the actor-thread death
/// signal (D7). `msg` is the captured panic message when the runtime could
/// downcast it (`&str` / `String` payloads); a non-string panic payload
/// degrades to a stable placeholder rather than dropping the frame.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PanicFrame {
    /// Thread panic message, best-effort. Always populated — never empty.
    pub msg: String,
}

/// Borrowing **emit-side** carrier for a discrete delta. Stamps
/// [`DELTA_SCHEMA_VERSION`] adjacent to the **flattened** [`KernelUpdate`], so
/// the on-wire `v` object is `{"schema_version":1,"<Variant>":{…}}` — the
/// version key sits beside the externally-tagged variant key, never nesting
/// it. This mirrors how the snapshot stamps `schema_version` alongside `rev`
/// and `last_tick_ms`, so both arms of the channel carry a version.
#[derive(Debug, Serialize)]
pub struct WireDelta<'a> {
    /// Discrete-update schema version — always [`DELTA_SCHEMA_VERSION`] on emit.
    pub schema_version: u8,
    /// The discrete update, flattened so its externally-tagged variant key
    /// sits beside `schema_version` rather than nested under it.
    #[serde(flatten)]
    pub update: &'a KernelUpdate,
}

/// Borrowing **emit-side** envelope. Used only to *serialize* a frame onto the
/// channel; the snapshot half borrows its already-serialized JSON so wrapping
/// never re-parses or clones the (large) payload (D8).
#[derive(Debug, Serialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum WireEnvelope<'a> {
    /// A discrete delta — a [`KernelUpdate`] stamped with [`DELTA_SCHEMA_VERSION`]
    /// (the `ActorCommand::Kernel` reducer result).
    Update(WireDelta<'a>),
    /// The periodic `Kernel::make_update` snapshot, already serialized.
    Snapshot(&'a RawValue),
    /// Actor-thread death (D7) — the kernel loop panicked or exited.
    Panic(&'a PanicFrame),
}

/// Owning **consumer-side** carrier for a discrete delta. The mirror of
/// [`WireDelta`]: the flattened [`KernelUpdate`] plus the discrete-update
/// schema version.
///
/// `schema_version` is `#[serde(default)]` — a pre-versioning frame (one
/// emitted before [`DELTA_SCHEMA_VERSION`] existed, i.e. with no
/// `schema_version` key) still decodes, defaulting to `1`. A host that does
/// not yet inspect the field is unaffected; a host that does can detect a
/// kernel-vs-shell delta-schema mismatch and degrade gracefully (D1) rather
/// than mis-applying a renamed/retyped variant.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DeltaEnvelope {
    /// Discrete-update schema version. Defaults to [`DELTA_SCHEMA_VERSION`]
    /// when absent so pre-versioning frames stay decodable.
    #[serde(default = "default_delta_schema_version")]
    pub schema_version: u8,
    /// The discrete update itself.
    #[serde(flatten)]
    pub update: KernelUpdate,
}

/// Owning **consumer-side** envelope. This is the single discriminated type
/// every host (and every `nmp-codegen`-projected shell) decodes the channel
/// into. The snapshot interior stays opaque ([`serde_json::Value`]) on
/// purpose: the contract this type models is the *discriminator*, not the
/// ~30-field snapshot's internals (which remain a crate-internal struct).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum UpdateEnvelope {
    /// A discrete kernel update — the host applies it as a delta. Wraps the
    /// [`KernelUpdate`] in a [`DeltaEnvelope`] so the delta arm carries a
    /// `schema_version` just as the snapshot arm does.
    Update(DeltaEnvelope),
    /// A full snapshot — the host replaces its rendered state.
    Snapshot(serde_json::Value),
    /// The actor thread died (panicked or exited). Terminal: the kernel is
    /// gone for this process. Hosts MUST surface a fatal error and stop
    /// sending commands — see the actor-death contract above.
    Panic(PanicFrame),
}

/// Serialize a discrete [`KernelUpdate`] as
/// `{"t":"update","v":{"schema_version":1,…}}`, stamping [`DELTA_SCHEMA_VERSION`]
/// alongside the flattened update — the delta-arm counterpart to how
/// `Kernel::make_update` stamps `schema_version` into every snapshot.
///
/// D6: serde never panics on these plain enums; a serialization failure
/// degrades to `None` (the caller drops the send) rather than unwinding
/// across the FFI seam.
pub fn wrap_update(update: &KernelUpdate) -> Option<String> {
    let delta = WireDelta {
        schema_version: DELTA_SCHEMA_VERSION,
        update,
    };
    serde_json::to_string(&WireEnvelope::Update(delta)).ok()
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
    use crate::app::KernelUpdate;

    /// The on-wire tag values MUST be exactly `"update"` / `"snapshot"`
    /// (snake_case), never the Rust variant casing — hosts pin these strings.
    #[test]
    fn tag_strings_are_snake_case_lowercase() {
        let u = KernelUpdate::ViewOpened {
            namespace: "profile".into(),
            key: "pk".into(),
        };
        let wire = wrap_update(&u).expect("update serializes");
        assert!(
            wire.starts_with(r#"{"t":"update","v":"#),
            "discrete frame must be tagged t=update: {wire}"
        );

        let snap = wrap_snapshot(r#"{"rev":7,"open_views":2}"#.to_string())
            .expect("snapshot serializes");
        assert!(
            snap.starts_with(r#"{"t":"snapshot","v":"#),
            "snapshot frame must be tagged t=snapshot: {snap}"
        );
    }

    /// Round-trip the **discrete** shape through the consumer envelope. The
    /// emitted frame carries `DELTA_SCHEMA_VERSION` and the inner update
    /// survives unchanged.
    #[test]
    fn round_trip_update_shape() {
        let original = KernelUpdate::UriRejected {
            uri: "nostr:nsec1bad".into(),
            reason: "unparseable nostr URI".into(),
        };
        let wire = wrap_update(&original).expect("serialize");
        let decoded: UpdateEnvelope = serde_json::from_str(&wire).expect("decode");
        match decoded {
            UpdateEnvelope::Update(d) => {
                assert_eq!(d.update, original);
                assert_eq!(d.schema_version, DELTA_SCHEMA_VERSION);
            }
            other => panic!("misclassified update frame: {other:?}"),
        }
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

    /// Consumer-side disambiguation: a single decoder distinguishes the two
    /// shapes purely by the `t` tag — never by sniffing payload keys. This is
    /// the contract every host relies on.
    #[test]
    fn consumer_disambiguates_two_shapes_on_one_channel() {
        let channel: Vec<String> = vec![
            wrap_update(&KernelUpdate::ViewOpened {
                namespace: "thread".into(),
                key: "evid".into(),
            })
            .unwrap(),
            wrap_snapshot(r#"{"rev":1,"open_views":1}"#.to_string()).unwrap(),
            wrap_update(&KernelUpdate::Started { rev: 0 }).unwrap(),
        ];

        let mut updates = 0usize;
        let mut snapshots = 0usize;
        for frame in &channel {
            match serde_json::from_str::<UpdateEnvelope>(frame).expect("decodes") {
                UpdateEnvelope::Update(_) => updates += 1,
                UpdateEnvelope::Snapshot(_) => snapshots += 1,
                UpdateEnvelope::Panic(p) => panic!("unexpected panic frame: {}", p.msg),
            }
        }
        assert_eq!(updates, 2, "two discrete updates on the channel");
        assert_eq!(snapshots, 1, "one snapshot on the channel");
    }

    /// **Backward compatibility:** a *pre-versioning* delta frame — emitted by
    /// a kernel that predates `DELTA_SCHEMA_VERSION`, so the `v` object has no
    /// `schema_version` key — must still decode, with `schema_version`
    /// defaulting to `DELTA_SCHEMA_VERSION`. This is the guarantee that makes
    /// the versioning change non-breaking for existing hosts.
    #[test]
    fn pre_versioning_delta_frame_decodes_with_default_version() {
        let wire = r#"{"t":"update","v":{"ViewOpened":{"namespace":"profile","key":"pk"}}}"#;
        let decoded: UpdateEnvelope = serde_json::from_str(wire).expect("decode");
        assert_eq!(
            decoded,
            UpdateEnvelope::Update(DeltaEnvelope {
                schema_version: DELTA_SCHEMA_VERSION,
                update: KernelUpdate::ViewOpened {
                    namespace: "profile".into(),
                    key: "pk".into(),
                },
            })
        );
    }

    /// Pins the **current** delta wire shape: `schema_version` sits flattened
    /// beside the externally-tagged `KernelUpdate` variant inside `v`, never
    /// nesting it. An accidental refactor that nests or drops the version key
    /// fails here.
    #[test]
    fn versioned_delta_wire_shape_is_pinned() {
        let wire = wrap_update(&KernelUpdate::ViewOpened {
            namespace: "profile".into(),
            key: "pk".into(),
        })
        .expect("serialize");
        assert_eq!(
            wire,
            r#"{"t":"update","v":{"schema_version":1,"ViewOpened":{"namespace":"profile","key":"pk"}}}"#,
            "delta wire shape changed: {wire}"
        );

        // Hand-written versioned bytes decode into the same value.
        let decoded: UpdateEnvelope = serde_json::from_str(&wire).expect("decode");
        assert_eq!(
            decoded,
            UpdateEnvelope::Update(DeltaEnvelope {
                schema_version: 1,
                update: KernelUpdate::ViewOpened {
                    namespace: "profile".into(),
                    key: "pk".into(),
                },
            })
        );
    }

    /// The delta schema version starts at `1`. A bump is a deliberate,
    /// breaking-change act; this guards against an accidental edit — the
    /// delta-arm counterpart to `snapshot_schema_version_is_one`.
    #[test]
    fn delta_schema_version_is_one() {
        assert_eq!(DELTA_SCHEMA_VERSION, 1);
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
