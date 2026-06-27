//! ADR-0064 §3 (#1783) — the action-builder registry: the single source of
//! truth describing every generated typed write builder.
//!
//! ## What this models
//!
//! Each [`ActionBuilder`] describes ONE app-facing typed write method
//! (`client.react(...)`, `client.follow(...)`, …): its GENERATED
//! `action_namespace` (the open-registry routing key — ADR-0064 §2, never
//! hand-written by app code) and the FlatBuffers payload table field shape. The
//! neutral namespace/schema/file-id facts live in
//! [`crate::action_contract::ACTION_CONTRACT`] so registration, public payload
//! exposure, and generated builders share one source.
//!
//! The Swift/Kotlin emitters ([`crate::action_builders::swift`] /
//! [`crate::action_builders::kotlin`]) read this slice and emit, per builder, a
//! typed method that:
//!
//! 1. encodes the per-crate FlatBuffers payload table (field order = the order
//!    declared here, which MUST match the `.fbs` table) directly via the
//!    FlatBuffers runtime builder — no flatc-generated payload class needed; and
//! 2. stamps `(namespace, DISPATCH_ENVELOPE_SCHEMA_VERSION, payload)` into a
//!    `DispatchEnvelope` and returns the finished bytes for the one byte doorway
//!    `nmp_app_dispatch_action_bytes` (#1752).
//!
//! ## Why the registry lives in `nmp-codegen` (D0 exemption)
//!
//! Like [`crate::swift_projections_registry`], this slice names dotted protocol
//! routing keys (`"nmp.nip25.react"`) and per-NIP field shapes. Those substrings
//! would trip D0 doctrine-lint in `nmp-core`; `nmp-codegen` is a host-side tool
//! crate, exempt from D0, and these are the actual wire keys/fields the host
//! builders construct — not kernel nouns.
//!
//! ## Field-order is load-bearing
//!
//! A FlatBuffers table field's slot is its DECLARATION ORDER in the `.fbs`. The
//! Rust decoder (`react_fb::ReactPayload` etc.) reads by vtable slot, so the
//! field order in [`ActionBuilder::fields`] MUST match the `.fbs` table exactly
//! — `schema_version` is always slot 0 (the fail-closed tripwire), then the
//! data fields. The round-trip test in `nmp-codegen` is NOT enough on its own;
//! the authoritative guard is the per-crate Rust `decode` + the round-trip
//! integration test that decodes builder-shaped bytes.

/// The single recognised envelope schema version, mirrored from
/// `nmp_core::dispatch_envelope::DISPATCH_ENVELOPE_SCHEMA_VERSION`. Kept as a
/// local literal (not a cross-crate import) so `nmp-codegen` keeps its
/// no-`nmp-core` posture; the round-trip test asserts the two agree.
pub const DISPATCH_ENVELOPE_SCHEMA_VERSION: u32 = 1;

/// The write envelope FlatBuffers file identifier, mirrored from
/// `nmp_core::dispatch_envelope::DISPATCH_ENVELOPE_FILE_IDENTIFIER_STR`.
pub const DISPATCH_ENVELOPE_FILE_IDENTIFIER: &str = "NMPD";

/// A scalar/offset field on a generated payload table.
///
/// The `kind` decides how the emitter encodes the field; `optional` controls
/// whether the generated method accepts `nil`/`null` and skips the field when
/// absent (FlatBuffers omits absent optional fields — the Rust decoder reads
/// them as `None`).
pub struct PayloadField {
    /// Field name on the generated builder method parameter + the FlatBuffers
    /// table (lowerCamelCase for the host API; the `.fbs` uses snake_case but
    /// FlatBuffers identifies fields by SLOT, not name, so only order matters).
    pub name: &'static str,
    /// How to encode this field.
    pub kind: FieldKind,
    /// When `true`, the generated method parameter is optional and the field is
    /// omitted from the buffer when absent (decoded as `None` in Rust).
    pub optional: bool,
}

/// The wire type of a [`PayloadField`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// A `string` field — encoded as an offset.
    Str,
    /// A `uint` (u32) scalar field — encoded inline with default `0`.
    Uint,
    /// A `[string]` vector field — encoded as a vector of string offsets.
    StrVec,
    /// A `ulong` (u64) scalar field — encoded inline with default `0`.
    Ulong,
    /// A `ulong` scalar + a companion `bool` presence flag (two FlatBuffers
    /// slots). Preserves `Some(0)` vs `None` for `Option<u64>` on the Rust
    /// side: the decoder reads `Some(v)` only when the flag is true.
    UlongWithPresenceFlag {
        /// Name of the companion bool presence-flag field (next slot).
        flag_name: &'static str,
    },
    /// A `[RelayListEntry]` vector: each entry is a FlatBuffers table with
    /// `url:string (required)` (slot 0) and `marker:RelayMarker` ubyte (slot 1,
    /// default 0 = Both). Role strings are mapped to bytes by the generated
    /// `relayMarkerByte` helper — no role logic in host code.
    RelayListEntryVec,
}

impl PayloadField {
    /// Number of FlatBuffers slots this field occupies (declaration-order in
    /// the `.fbs`). All types are 1 except `UlongWithPresenceFlag`, which
    /// occupies 2 (the scalar + the bool flag).
    pub fn slot_count(&self) -> usize {
        match self.kind {
            FieldKind::UlongWithPresenceFlag { .. } => 2,
            _ => 1,
        }
    }
}

/// One generated typed write builder.
pub struct ActionBuilder {
    /// The GENERATED open-registry routing key stamped into the
    /// `DispatchEnvelope.action_namespace` (e.g. `"nmp.nip25.react"`). Never
    /// hand-written by app code — it exists only inside generated code.
    pub namespace: &'static str,
    /// The host-facing method name (`react`, `follow`, `followMany`, …),
    /// lowerCamelCase. Emitted verbatim in both Swift and Kotlin (both use
    /// lowerCamelCase method names).
    pub method: &'static str,
    /// The payload table fields in DECLARATION ORDER (slot order). Slot 0 is the
    /// implicit `schema_version` tripwire — it is NOT listed here; the emitter
    /// always writes the contract's schema version at slot 0 and these fields
    /// start at slot 1.
    pub fields: &'static [PayloadField],
    /// One-line human description for the generated doc comment.
    pub doc: &'static str,
}

// The `ACTION_BUILDERS` flat-table data lives in the `table` submodule (split
// out to keep this file under the 500-LOC ceiling); re-exported so consumers
// keep using `registry::ACTION_BUILDERS`.
mod table;
pub use table::ACTION_BUILDERS;

// ── nmp.publish — the UNION-bodied builders (ADR-0064 §3) ────────────────────
//
// `nmp.publish` is the namespace EVERY second-app consumer (hl / tenex-off /
// podcast, iOS + Android) actually writes through, so the typed builders don't
// unblock those migrations without it. Unlike the flat tables above, the
// `PublishPayload` root wraps a UNION body (`publish.fbs`):
//
//   table PublishPayload {        // root, file_identifier "NPUB"
//     schema_version:uint;        // slot 0 — fail-closed tripwire (vtable 4)
//     body:PublishPayloadBody (required); // union → body_type at slot 1
//   }                                     //         body offset at slot 2
//   union PublishPayloadBody { PublishSigned, PublishProfile, PublishRaw }
//
// A FlatBuffers union expands to TWO root fields: a `*_type` ubyte discriminant
// (declaration-order: NONE=0, PublishSigned=1, PublishProfile=2, PublishRaw=3)
// and the body table offset. The emitters build the nested body table first,
// then stamp `(schema_version, body_type, body)` into the root — the byte-for-
// byte twin of `encode_publish_payload` in `nmp-core/src/publish/wire.rs`.
//
// Modelling the body shapes as data (rather than special-casing each in the
// emitter) keeps the Swift/Kotlin emitters' publish paths a single shared
// template. `PublishSigned` is intentionally NOT a builder: its body carries
// the OPAQUE canonical NIP-01 bytes a signer produced (signature byte-
// exactness — never a typed re-encode), which is a pre-signed-event handoff,
// not a typed-field write. The two typed-field variants consumers use —
// `PublishRaw` (generic publish) and `PublishProfile` (kind:0 metadata) — are
// the builders.

/// The union member discriminant (declaration order in `union
/// PublishPayloadBody`, with NONE=0). Stamped as the `body_type` ubyte at the
/// `PublishPayload` root.
pub const PUBLISH_BODY_PUBLISH_PROFILE: u8 = 2;
/// See [`PUBLISH_BODY_PUBLISH_PROFILE`].
pub const PUBLISH_BODY_PUBLISH_RAW: u8 = 3;

/// One `nmp.publish` union-bodied builder.
///
/// Each maps to one `PublishPayloadBody` variant. The emitters hand-roll the
/// nested body table from [`PublishBuilder::body`] (a `BodyShape`), then wrap it
/// in the `PublishPayload` root with [`PublishBuilder::body_type`].
pub struct PublishBuilder {
    /// Host-facing method name (`publishRaw`, `publishProfile`).
    pub method: &'static str,
    /// The union discriminant for this body (`PUBLISH_BODY_*`).
    pub body_type: u8,
    /// The body table shape the emitter encodes.
    pub body: BodyShape,
    /// One-line doc for the generated method.
    pub doc: &'static str,
}

/// The shape of a `PublishPayloadBody` variant's body table. Each variant is a
/// distinct FlatBuffers table layout, so this is a closed enum the emitter
/// matches on (not a generic field list — the publish body tables nest other
/// tables: `PublishTarget`, `TagRow`, `ProfileField`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyShape {
    /// `PublishRaw { kind:uint, tags:[TagRow], content:string,
    /// target:PublishTarget, signer_pubkey:string }`.
    PublishRaw,
    /// `PublishProfile { fields:[ProfileField] }`.
    PublishProfile,
}

/// The `nmp.publish` builders. See [`PublishBuilder`].
pub const PUBLISH_BUILDERS: &[PublishBuilder] = &[
    PublishBuilder {
        method: "publishRaw",
        body_type: PUBLISH_BODY_PUBLISH_RAW,
        body: BodyShape::PublishRaw,
        doc: "Sign-and-publish an arbitrary event kind (generic publish path; NIP-65 outbox or explicit relays).",
    },
    PublishBuilder {
        method: "publishProfile",
        body_type: PUBLISH_BODY_PUBLISH_PROFILE,
        body: BodyShape::PublishProfile,
        doc: "Sign-and-publish a kind:0 profile metadata event for the active account.",
    },
];
