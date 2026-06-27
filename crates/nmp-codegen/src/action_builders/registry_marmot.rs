//! M14-1c / #2169 — the `nmp.marmot` union-builder registry slice.
//!
//! Split out of [`crate::action_builders::registry`] purely as a size-
//! management seam (AGENTS.md / V-12): the flat-table + publish-union registry
//! already fills `registry.rs`, and the 9-arm Marmot union pushed it past the
//! 500-LOC ceiling. The types here are re-exported from `registry` (`pub use
//! registry_marmot::*;`) so callers see one flat `registry::` surface.
//!
//! `nmp.marmot` carries a FlatBuffers UNION body (one nested arm body table +
//! a union discriminant), exactly like `nmp.publish` ([`crate::action_builders::registry::PUBLISH_BUILDERS`]).
//! The emitters (`swift_marmot` / `kotlin_marmot` / `ts_marmot`) hand-roll each
//! arm body and wrap it in the `MarmotActionPayload` root — the byte-for-byte
//! twin of `MarmotAction::encode` in `nmp_marmot::wire::action_payload`.

/// The `nmp.marmot` union namespace (mirrored from `nmp_marmot::MARMOT_ACTION_NAMESPACE`).
/// Kept as a codegen-local literal so `nmp-codegen` stays dep-free from `nmp-marmot`.
pub const MARMOT_NAMESPACE: &str = "nmp.marmot";

// Union discriminant constants — MUST match the declaration order in
// `crates/nmp-marmot/schema/marmot_action.fbs` (NONE=0 is never used).
pub const MARMOT_BODY_PUBLISH_KEY_PACKAGE: u8 = 1;
pub const MARMOT_BODY_CREATE_GROUP: u8 = 2;
pub const MARMOT_BODY_INVITE: u8 = 3;
pub const MARMOT_BODY_SEND: u8 = 4;
pub const MARMOT_BODY_LEAVE: u8 = 5;
pub const MARMOT_BODY_REMOVE: u8 = 6;
pub const MARMOT_BODY_ACCEPT_WELCOME: u8 = 7;
pub const MARMOT_BODY_DECLINE_WELCOME: u8 = 8;
pub const MARMOT_BODY_CLEAR_PENDING: u8 = 9;

/// One `nmp.marmot` union-bodied builder.
///
/// Maps to one `MarmotActionBody` arm. Emitters hand-roll the arm's body table
/// from the `body` discriminant, then wrap it in the `MarmotActionPayload` root.
pub struct MarmotBuilder {
    /// Host-facing method name (lowerCamelCase). Emitted verbatim in Swift/Kotlin/TS.
    pub method: &'static str,
    /// The union discriminant for this arm (`MARMOT_BODY_*`).
    pub body_type: u8,
    /// The arm body's table shape.
    pub body: MarmotBodyShape,
    /// One-line doc for the generated method.
    pub doc: &'static str,
}

/// The shape of a `MarmotActionBody` arm's body table. Each variant is a
/// distinct FlatBuffers table layout matched by the emitter.
///
/// Emitters MUST preserve field/slot order exactly as in `marmot_action.fbs`.
/// `schema_version` at slot 0 (vt 4) is the root field — emitters write it
/// once on the `MarmotActionPayload` root, NOT on each arm body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarmotBodyShape {
    /// `PublishKeyPackage { relays:[string] }` (slot 0 / vt 4 on the body table).
    PublishKeyPackage,
    /// `CreateGroup { name:string (required), description:string?,
    ///  invitee_text:string?, invitee_npubs:[string]?,
    ///  signed_key_package_events_json:[string]?, relays:[string] }`.
    ///
    /// `invitee_npubs` is a presence-flagged optional `[string]`:
    /// `None`→absent offset, `Some([])` → present empty vector, `Some(v)` → populated.
    CreateGroup,
    /// `Invite { group_id_hex:string (required), invitee_text:string?,
    ///  invitee_npubs:[string]?, signed_key_package_events_json:[string]? }`.
    Invite,
    /// `Send { group_id_hex:string (required), text:string (required) }`.
    Send,
    /// `Leave { group_id_hex:string (required) }`.
    Leave,
    /// `Remove { group_id_hex:string (required), member_npubs:[string] }`.
    Remove,
    /// `AcceptWelcome { welcome_id_hex:string (required) }`.
    AcceptWelcome,
    /// `DeclineWelcome { welcome_id_hex:string (required) }`.
    DeclineWelcome,
    /// `ClearPending { group_id_hex:string (required) }`.
    ClearPending,
}

/// The `nmp.marmot` union-bodied builders. One entry per `MarmotActionBody` arm.
pub const MARMOT_BUILDERS: &[MarmotBuilder] = &[
    MarmotBuilder {
        method: "marmotPublishKeyPackage",
        body_type: MARMOT_BODY_PUBLISH_KEY_PACKAGE,
        body: MarmotBodyShape::PublishKeyPackage,
        doc: "Publish (or rotate) the local MLS key-package (kind:30443) to relays.",
    },
    MarmotBuilder {
        method: "marmotCreateGroup",
        body_type: MARMOT_BODY_CREATE_GROUP,
        body: MarmotBodyShape::CreateGroup,
        doc: "Create a new MLS group and optionally invite peers.",
    },
    MarmotBuilder {
        method: "marmotInvite",
        body_type: MARMOT_BODY_INVITE,
        body: MarmotBodyShape::Invite,
        doc: "Invite one or more peers to an existing MLS group.",
    },
    MarmotBuilder {
        method: "marmotSend",
        body_type: MARMOT_BODY_SEND,
        body: MarmotBodyShape::Send,
        doc: "Send a kind:14 NIP-44 MLS group message.",
    },
    MarmotBuilder {
        method: "marmotLeave",
        body_type: MARMOT_BODY_LEAVE,
        body: MarmotBodyShape::Leave,
        doc: "Self-remove from a MLS group (SelfRemove proposal + commit).",
    },
    MarmotBuilder {
        method: "marmotRemove",
        body_type: MARMOT_BODY_REMOVE,
        body: MarmotBodyShape::Remove,
        doc: "Remove other members from a MLS group (Remove proposal + commit).",
    },
    MarmotBuilder {
        method: "marmotAcceptWelcome",
        body_type: MARMOT_BODY_ACCEPT_WELCOME,
        body: MarmotBodyShape::AcceptWelcome,
        doc: "Accept a pending MLS Welcome (by gift-wrap event id hex).",
    },
    MarmotBuilder {
        method: "marmotDeclineWelcome",
        body_type: MARMOT_BODY_DECLINE_WELCOME,
        body: MarmotBodyShape::DeclineWelcome,
        doc: "Decline a pending MLS Welcome.",
    },
    MarmotBuilder {
        method: "marmotClearPending",
        body_type: MARMOT_BODY_CLEAR_PENDING,
        body: MarmotBodyShape::ClearPending,
        doc: "Explicitly clear the pending-commit state for a MLS group.",
    },
];
