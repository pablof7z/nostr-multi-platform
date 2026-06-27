//! ADR-0064 / #2169 (M14-1c) — typed FlatBuffers payload codec for the
//! `nmp.marmot` `ActionModule` (the MLS-over-Nostr write seam).
//!
//! Implements [`ActionPayload`] for [`MarmotAction`] so the byte doorway can
//! route `nmp.marmot` dispatches through
//! [`MarmotActionModule::decode_payload`] (added in this PR). The encode path
//! is the Rust primitive round-trip codec that the host builders must be
//! byte-exact with; the decode path is the fail-closed registry adapter.
//!
//! # Schema
//!
//! `marmot_action.fbs` defines a `MarmotActionPayload` root that wraps a
//! `MarmotActionBody` union (9 arms). The union discriminant (ubyte) selects
//! the arm; slot 0 is always the `schema_version` fail-closed tripwire.
//!
//! # Lossless round-trip
//!
//! Every field of every `MarmotAction` arm encodes into the schema and decodes
//! back unchanged:
//! * `Option<String>` → absent / present FlatBuffers string offset.
//! * `Option<Vec<String>>` → absent / present `[string]` vector (distinguishes
//!   `None` from `Some(vec![])` via the offset-vs-count distinction).
//! * `Vec<serde_json::Value>` → `[string]` where each element is
//!   `serde_json::Value::to_string()`, decoded via `serde_json::from_str`.
//!   Empty `Vec` encodes as absent; decode reconstructs `vec![]` for absent.
//! * `Vec<String>` with `#[serde(default)]` → `[string]`; absent → `vec![]`.
//!
//! Honours D6: decode returns a data-shaped [`ActionPayloadDecodeError`] on any
//! malformed input; no panics on the decode path.

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "generated/marmot_action_generated.rs"]
pub mod generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use generated::nmp::marmot as fb;

use crate::projection::action::MarmotAction;

/// Wire schema version for the marmot action payload. Bump on any breaking
/// change to `marmot_action.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Encode a NON-OPTIONAL `Vec<String>` as a FlatBuffers `[string]` offset,
/// ALWAYS present (even when empty → a present empty vector). This matches the
/// canonical convention for non-optional `[string]` fields used by the
/// generated host builders (Swift/Kotlin/TS) AND by the `nmp-nip02`
/// `FollowManyAction` encoder (`fbb.create_vector` is unconditional there), so
/// Rust-encode and host-encode produce byte-identical buffers (golden-fixture
/// parity — #2169). The decoder reads absent and present-empty identically
/// (both → `vec![]`), so this is purely about cross-encoder byte parity.
fn encode_str_vec<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    v: &[String],
) -> Option<flatbuffers::WIPOffset<flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<&'a str>>>>
{
    let offsets: Vec<_> = v.iter().map(|s| fbb.create_string(s)).collect();
    Some(fbb.create_vector(&offsets))
}

/// Encode an `Option<Vec<String>>` preserving `Some([])` vs `None`.
/// `None` → absent (no offset).  `Some([])` → present empty vector.
/// `Some(v)` → present non-empty vector.
fn encode_opt_str_vec<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    v: &Option<Vec<String>>,
) -> Option<flatbuffers::WIPOffset<flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<&'a str>>>>
{
    let inner = v.as_ref()?;
    let offsets: Vec<_> = inner.iter().map(|s| fbb.create_string(s)).collect();
    Some(fbb.create_vector(&offsets))
}

/// Encode a NON-OPTIONAL `Vec<serde_json::Value>` as `[string]`, ALWAYS present
/// (even when empty → a present empty vector — see [`encode_str_vec`] for the
/// byte-parity rationale). Each Value is serialised via `to_string()`.
fn encode_json_vec<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    v: &[serde_json::Value],
) -> Option<flatbuffers::WIPOffset<flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<&'a str>>>>
{
    let strs: Vec<String> = v.iter().map(|x| x.to_string()).collect();
    let offsets: Vec<_> = strs.iter().map(|s| fbb.create_string(s)).collect();
    Some(fbb.create_vector(&offsets))
}

/// Decode a FlatBuffers optional `[string]` vector to `Vec<String>`.
/// Absent → `vec![]` (the `#[serde(default)]` equivalent).
fn decode_str_vec(
    v: Option<flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<&'_ str>>>,
) -> Vec<String> {
    v.map(|vec| vec.iter().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Decode a FlatBuffers optional `[string]` to `Option<Vec<String>>`.
/// Absent → `None`.  Present (even if empty) → `Some(...)`.
fn decode_opt_str_vec(
    v: Option<flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<&'_ str>>>,
) -> Option<Vec<String>> {
    v.map(|vec| vec.iter().map(str::to_string).collect())
}

/// Decode a FlatBuffers optional `[string]` to `Vec<serde_json::Value>`.
/// Each string is parsed via `serde_json::from_str`. Absent → `vec![]`.
fn decode_json_vec(
    v: Option<flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<&'_ str>>>,
) -> Result<Vec<serde_json::Value>, ActionPayloadDecodeError> {
    match v {
        None => Ok(vec![]),
        Some(vec) => vec
            .iter()
            .map(|s| {
                serde_json::from_str(s).map_err(|e| {
                    malformed(format!(
                        "signed_key_package_events_json element is not valid JSON: {e}"
                    ))
                })
            })
            .collect(),
    }
}

// ── ActionPayload impl ────────────────────────────────────────────────────────

impl ActionPayload for MarmotAction {
    const SCHEMA_ID: &'static str = "nmp.marmot";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();

        // Union discriminant + body offset. Build the body table FIRST (FlatBuffers
        // requires nested objects before the table that references them).
        let (body_type, body_offset): (fb::MarmotActionBody, flatbuffers::WIPOffset<flatbuffers::UnionWIPOffset>) = match self {
            // ── PublishKeyPackage ────────────────────────────────────────────
            MarmotAction::PublishKeyPackage { relays } => {
                let relays_off = encode_str_vec(&mut fbb, relays);
                let t = fb::PublishKeyPackage::create(
                    &mut fbb,
                    &fb::PublishKeyPackageArgs { relays: relays_off },
                );
                (fb::MarmotActionBody::PublishKeyPackage, t.as_union_value())
            }

            // ── CreateGroup ──────────────────────────────────────────────────
            MarmotAction::CreateGroup {
                name,
                description,
                invitee_text,
                invitee_npubs,
                signed_key_package_events_json,
                relays,
            } => {
                let relays_off = encode_str_vec(&mut fbb, relays);
                let json_off = encode_json_vec(&mut fbb, signed_key_package_events_json);
                let npubs_off = encode_opt_str_vec(&mut fbb, invitee_npubs);
                let text_off = invitee_text.as_deref().map(|s| fbb.create_string(s));
                let desc_off = if description.is_empty() {
                    None
                } else {
                    Some(fbb.create_string(description))
                };
                let name_off = fbb.create_string(name);
                let t = fb::CreateGroup::create(
                    &mut fbb,
                    &fb::CreateGroupArgs {
                        name: Some(name_off),
                        description: desc_off,
                        invitee_text: text_off,
                        invitee_npubs: npubs_off,
                        signed_key_package_events_json: json_off,
                        relays: relays_off,
                    },
                );
                (fb::MarmotActionBody::CreateGroup, t.as_union_value())
            }

            // ── Invite ───────────────────────────────────────────────────────
            MarmotAction::Invite {
                group_id_hex,
                invitee_text,
                invitee_npubs,
                signed_key_package_events_json,
            } => {
                let json_off = encode_json_vec(&mut fbb, signed_key_package_events_json);
                let npubs_off = encode_opt_str_vec(&mut fbb, invitee_npubs);
                let text_off = invitee_text.as_deref().map(|s| fbb.create_string(s));
                let gid_off = fbb.create_string(group_id_hex);
                let t = fb::Invite::create(
                    &mut fbb,
                    &fb::InviteArgs {
                        group_id_hex: Some(gid_off),
                        invitee_text: text_off,
                        invitee_npubs: npubs_off,
                        signed_key_package_events_json: json_off,
                    },
                );
                (fb::MarmotActionBody::Invite, t.as_union_value())
            }

            // ── Send ─────────────────────────────────────────────────────────
            MarmotAction::Send { group_id_hex, text } => {
                let text_off = fbb.create_string(text);
                let gid_off = fbb.create_string(group_id_hex);
                let t = fb::Send::create(
                    &mut fbb,
                    &fb::SendArgs {
                        group_id_hex: Some(gid_off),
                        text: Some(text_off),
                    },
                );
                (fb::MarmotActionBody::Send, t.as_union_value())
            }

            // ── Leave ────────────────────────────────────────────────────────
            MarmotAction::Leave { group_id_hex } => {
                let gid_off = fbb.create_string(group_id_hex);
                let t = fb::Leave::create(
                    &mut fbb,
                    &fb::LeaveArgs { group_id_hex: Some(gid_off) },
                );
                (fb::MarmotActionBody::Leave, t.as_union_value())
            }

            // ── Remove ───────────────────────────────────────────────────────
            MarmotAction::Remove { group_id_hex, member_npubs } => {
                let npubs_off = encode_str_vec(&mut fbb, member_npubs);
                let gid_off = fbb.create_string(group_id_hex);
                let t = fb::Remove::create(
                    &mut fbb,
                    &fb::RemoveArgs {
                        group_id_hex: Some(gid_off),
                        member_npubs: npubs_off,
                    },
                );
                (fb::MarmotActionBody::Remove, t.as_union_value())
            }

            // ── AcceptWelcome ────────────────────────────────────────────────
            MarmotAction::AcceptWelcome { welcome_id_hex } => {
                let wid_off = fbb.create_string(welcome_id_hex);
                let t = fb::AcceptWelcome::create(
                    &mut fbb,
                    &fb::AcceptWelcomeArgs { welcome_id_hex: Some(wid_off) },
                );
                (fb::MarmotActionBody::AcceptWelcome, t.as_union_value())
            }

            // ── DeclineWelcome ───────────────────────────────────────────────
            MarmotAction::DeclineWelcome { welcome_id_hex } => {
                let wid_off = fbb.create_string(welcome_id_hex);
                let t = fb::DeclineWelcome::create(
                    &mut fbb,
                    &fb::DeclineWelcomeArgs { welcome_id_hex: Some(wid_off) },
                );
                (fb::MarmotActionBody::DeclineWelcome, t.as_union_value())
            }

            // ── ClearPending ─────────────────────────────────────────────────
            MarmotAction::ClearPending { group_id_hex } => {
                let gid_off = fbb.create_string(group_id_hex);
                let t = fb::ClearPending::create(
                    &mut fbb,
                    &fb::ClearPendingArgs { group_id_hex: Some(gid_off) },
                );
                (fb::MarmotActionBody::ClearPending, t.as_union_value())
            }
        };

        // Root: schema_version (slot 0 / vt 4), body_type (slot 1 / vt 6), body
        // (slot 2 / vt 8). 3 fields in the root table.
        let root = fb::MarmotActionPayload::create(
            &mut fbb,
            &fb::MarmotActionPayloadArgs {
                schema_version: SCHEMA_VERSION,
                body_type,
                body: Some(body_offset),
            },
        );
        fb::finish_marmot_action_payload_buffer(&mut fbb, root);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::marmot_action_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NMMA file identifier"));
        }
        let root = fb::root_as_marmot_action_payload(bytes)
            .map_err(|e| malformed(format!("not a valid MarmotActionPayload buffer: {e}")))?;

        // Gate FIRST — schema_version BEFORE touching the union body.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }

        match root.body_type() {
            // ── PublishKeyPackage ────────────────────────────────────────────
            fb::MarmotActionBody::PublishKeyPackage => {
                let body = root
                    .body_as_publish_key_package()
                    .ok_or_else(|| malformed("PublishKeyPackage body table missing"))?;
                Ok(MarmotAction::PublishKeyPackage {
                    relays: decode_str_vec(body.relays()),
                })
            }

            // ── CreateGroup ──────────────────────────────────────────────────
            fb::MarmotActionBody::CreateGroup => {
                let body = root
                    .body_as_create_group()
                    .ok_or_else(|| malformed("CreateGroup body table missing"))?;
                Ok(MarmotAction::CreateGroup {
                    name: body.name().to_string(),
                    description: body.description().unwrap_or("").to_string(),
                    invitee_text: body.invitee_text().map(str::to_string),
                    invitee_npubs: decode_opt_str_vec(body.invitee_npubs()),
                    signed_key_package_events_json: decode_json_vec(
                        body.signed_key_package_events_json(),
                    )?,
                    relays: decode_str_vec(body.relays()),
                })
            }

            // ── Invite ───────────────────────────────────────────────────────
            fb::MarmotActionBody::Invite => {
                let body = root
                    .body_as_invite()
                    .ok_or_else(|| malformed("Invite body table missing"))?;
                Ok(MarmotAction::Invite {
                    group_id_hex: body.group_id_hex().to_string(),
                    invitee_text: body.invitee_text().map(str::to_string),
                    invitee_npubs: decode_opt_str_vec(body.invitee_npubs()),
                    signed_key_package_events_json: decode_json_vec(
                        body.signed_key_package_events_json(),
                    )?,
                })
            }

            // ── Send ─────────────────────────────────────────────────────────
            fb::MarmotActionBody::Send => {
                let body = root
                    .body_as_send()
                    .ok_or_else(|| malformed("Send body table missing"))?;
                Ok(MarmotAction::Send {
                    group_id_hex: body.group_id_hex().to_string(),
                    text: body.text().to_string(),
                })
            }

            // ── Leave ────────────────────────────────────────────────────────
            fb::MarmotActionBody::Leave => {
                let body = root
                    .body_as_leave()
                    .ok_or_else(|| malformed("Leave body table missing"))?;
                Ok(MarmotAction::Leave {
                    group_id_hex: body.group_id_hex().to_string(),
                })
            }

            // ── Remove ───────────────────────────────────────────────────────
            fb::MarmotActionBody::Remove => {
                let body = root
                    .body_as_remove()
                    .ok_or_else(|| malformed("Remove body table missing"))?;
                Ok(MarmotAction::Remove {
                    group_id_hex: body.group_id_hex().to_string(),
                    member_npubs: decode_str_vec(body.member_npubs()),
                })
            }

            // ── AcceptWelcome ────────────────────────────────────────────────
            fb::MarmotActionBody::AcceptWelcome => {
                let body = root
                    .body_as_accept_welcome()
                    .ok_or_else(|| malformed("AcceptWelcome body table missing"))?;
                Ok(MarmotAction::AcceptWelcome {
                    welcome_id_hex: body.welcome_id_hex().to_string(),
                })
            }

            // ── DeclineWelcome ───────────────────────────────────────────────
            fb::MarmotActionBody::DeclineWelcome => {
                let body = root
                    .body_as_decline_welcome()
                    .ok_or_else(|| malformed("DeclineWelcome body table missing"))?;
                Ok(MarmotAction::DeclineWelcome {
                    welcome_id_hex: body.welcome_id_hex().to_string(),
                })
            }

            // ── ClearPending ─────────────────────────────────────────────────
            fb::MarmotActionBody::ClearPending => {
                let body = root
                    .body_as_clear_pending()
                    .ok_or_else(|| malformed("ClearPending body table missing"))?;
                Ok(MarmotAction::ClearPending {
                    group_id_hex: body.group_id_hex().to_string(),
                })
            }

            other => Err(malformed(format!(
                "unknown MarmotActionBody discriminant: {}",
                other.0
            ))),
        }
    }
}

#[cfg(test)]
#[path = "action_payload_tests.rs"]
mod tests;
