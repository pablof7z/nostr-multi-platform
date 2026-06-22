//! ADR-0064 §3 (#1783) — the action-builder registry: the single source of
//! truth describing every generated typed write builder.
//!
//! ## What this models
//!
//! Each [`ActionBuilder`] describes ONE app-facing typed write method
//! (`client.react(...)`, `client.follow(...)`, …): its GENERATED
//! `action_namespace` (the open-registry routing key — ADR-0064 §2, never
//! hand-written by app code), the FlatBuffers payload table that namespace's
//! `ActionModule` decodes (S3 / #1751), and the `schema_version` stamped on
//! both the payload and the wrapping [`DispatchEnvelope`].
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
    /// The FlatBuffers payload table's file identifier (`"N25R"`, `"NF2A"`, …),
    /// stamped into the finished payload buffer so the per-crate Rust decoder's
    /// `*_buffer_has_identifier` gate passes.
    pub payload_file_identifier: &'static str,
    /// The payload schema version stamped into the payload table's slot-0
    /// `schema_version` field (the per-crate `SCHEMA_VERSION`, currently 1 for
    /// every S3 module).
    pub payload_schema_version: u32,
    /// The payload table fields in DECLARATION ORDER (slot order). Slot 0 is the
    /// implicit `schema_version` tripwire — it is NOT listed here; the emitter
    /// always writes it at slot 0 and these fields start at slot 1.
    pub fields: &'static [PayloadField],
    /// One-line human description for the generated doc comment.
    pub doc: &'static str,
}

/// The S3 module trio's flat-table builders (ADR-0064 §3 acceptance scope).
///
/// `nmp.publish` carries a FlatBuffers UNION body (`PublishSigned` /
/// `PublishProfile` / `PublishRaw`) rather than a flat table; its builders are a
/// deliberate follow-up (the union encode is materially more involved than the
/// flat-table S3 members and warrants its own slice — see the issue's
/// STOP-and-report). This registry covers the five flat-table namespaces
/// end-to-end: every primitive (string, uint, optional string, string vector)
/// is exercised, so the publish union is the only encode shape left.
pub const ACTION_BUILDERS: &[ActionBuilder] = &[
    // nip25 — react / unreact (react.fbs / unreact.fbs).
    ActionBuilder {
        namespace: "nmp.nip25.react",
        method: "react",
        payload_file_identifier: "N25R",
        payload_schema_version: 1,
        fields: &[
            PayloadField { name: "targetEventId", kind: FieldKind::Str, optional: false },
            PayloadField { name: "reaction", kind: FieldKind::Str, optional: false },
            PayloadField { name: "targetAuthorPubkey", kind: FieldKind::Str, optional: true },
        ],
        doc: "Publish a NIP-25 reaction to a target event.",
    },
    ActionBuilder {
        namespace: "nmp.nip25.unreact",
        method: "unreact",
        payload_file_identifier: "N25U",
        payload_schema_version: 1,
        fields: &[
            PayloadField { name: "reactionEventId", kind: FieldKind::Str, optional: false },
            PayloadField { name: "reason", kind: FieldKind::Str, optional: false },
        ],
        doc: "Retract a previously-published NIP-25 reaction.",
    },
    // nip02 — follow / unfollow share the single-pubkey FollowActionPayload
    // shape (follow_action.fbs); follow_many is the bulk primitive
    // (follow_many_action.fbs).
    ActionBuilder {
        namespace: "nmp.follow",
        method: "follow",
        payload_file_identifier: "NF2A",
        payload_schema_version: 1,
        fields: &[PayloadField { name: "pubkey", kind: FieldKind::Str, optional: false }],
        doc: "Follow a single pubkey (NIP-02 contact-list add).",
    },
    ActionBuilder {
        namespace: "nmp.unfollow",
        method: "unfollow",
        payload_file_identifier: "NF2A",
        payload_schema_version: 1,
        fields: &[PayloadField { name: "pubkey", kind: FieldKind::Str, optional: false }],
        doc: "Unfollow a single pubkey (NIP-02 contact-list remove).",
    },
    ActionBuilder {
        namespace: "nmp.follow_many",
        method: "followMany",
        payload_file_identifier: "NFMA",
        payload_schema_version: 1,
        fields: &[PayloadField { name: "pubkeys", kind: FieldKind::StrVec, optional: true }],
        doc: "Follow many pubkeys in one race-free read-modify-write cycle (NIP-02).",
    },
];
