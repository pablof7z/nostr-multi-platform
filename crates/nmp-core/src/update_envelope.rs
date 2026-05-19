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
//! ```
//!
//! so every consumer decodes exactly **one** discriminated type ([`UpdateEnvelope`]).
//!
//! D6 (FFI clean): the tag *is* the discriminant — no exceptions, no key
//! sniffing. D8 (no extra per-event alloc): the snapshot is already a
//! serialized `String`; it is re-attached by **borrowed** [`RawValue`] (no
//! re-parse, no clone of the payload), so wrapping costs a single outer
//! allocation.

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::app::KernelUpdate;

/// Borrowing **emit-side** envelope. Used only to *serialize* a frame onto the
/// channel; the snapshot half borrows its already-serialized JSON so wrapping
/// never re-parses or clones the (large) payload (D8).
#[derive(Debug, Serialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum WireEnvelope<'a> {
    /// A discrete [`KernelUpdate`] (the `ActorCommand::Kernel` reducer result).
    Update(&'a KernelUpdate),
    /// The periodic `Kernel::make_update` snapshot, already serialized.
    Snapshot(&'a RawValue),
}

/// Owning **consumer-side** envelope. This is the single discriminated type
/// every host (and every `nmp-codegen`-projected shell) decodes the channel
/// into. The snapshot interior stays opaque ([`serde_json::Value`]) on
/// purpose: the contract this type models is the *discriminator*, not the
/// ~30-field snapshot's internals (which remain a crate-internal struct).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum UpdateEnvelope {
    /// A discrete kernel update — the host applies it as a delta.
    Update(KernelUpdate),
    /// A full snapshot — the host replaces its rendered state.
    Snapshot(serde_json::Value),
}

/// Serialize a discrete [`KernelUpdate`] as `{"t":"update","v":…}`.
///
/// D6: serde never panics on these plain enums; a serialization failure
/// degrades to `None` (the caller drops the send) rather than unwinding
/// across the FFI seam.
pub fn wrap_update(update: &KernelUpdate) -> Option<String> {
    serde_json::to_string(&WireEnvelope::Update(update)).ok()
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

    /// Round-trip the **discrete** shape through the consumer envelope.
    #[test]
    fn round_trip_update_shape() {
        let original = KernelUpdate::UriRejected {
            uri: "nostr:nsec1bad".into(),
            reason: "unparseable nostr URI".into(),
        };
        let wire = wrap_update(&original).expect("serialize");
        let decoded: UpdateEnvelope = serde_json::from_str(&wire).expect("decode");
        match decoded {
            UpdateEnvelope::Update(u) => assert_eq!(u, original),
            UpdateEnvelope::Snapshot(_) => panic!("misclassified as snapshot"),
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
            UpdateEnvelope::Update(_) => panic!("misclassified as update"),
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
            }
        }
        assert_eq!(updates, 2, "two discrete updates on the channel");
        assert_eq!(snapshots, 1, "one snapshot on the channel");
    }

    /// Hand-written wire bytes must parse — pins the format so an accidental
    /// `rename`/variant rename can never silently change the contract.
    #[test]
    fn hand_written_wire_bytes_decode() {
        let wire = r#"{"t":"update","v":{"ViewOpened":{"namespace":"profile","key":"pk"}}}"#;
        let decoded: UpdateEnvelope = serde_json::from_str(wire).expect("decode");
        assert_eq!(
            decoded,
            UpdateEnvelope::Update(KernelUpdate::ViewOpened {
                namespace: "profile".into(),
                key: "pk".into(),
            })
        );
    }
}
