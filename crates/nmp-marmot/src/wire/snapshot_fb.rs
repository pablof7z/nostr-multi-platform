//! Typed FlatBuffers wire codec for [`crate::projection::payload::MarmotSnapshot`].
//!
//! The authoritative FFI shape of the `nmp.marmot.snapshot` projection is the
//! serde JSON of [`MarmotSnapshot`] (registered via `register_snapshot_projection`
//! in `crate::ffi::register_with_keys`). This module adds a **typed FlatBuffers**
//! encoding of the same struct — a self-describing, schema-versioned,
//! language-neutral binary the host platforms (Swift / Kotlin / TypeScript) can
//! decode with generated accessors instead of JSON reflection. It is a sidecar
//! codec: the serde shape stays authoritative; this is the typed payload carried
//! in the `typed_projections` sidecar (ADR-0037,
//! `crates/nmp-core/schema/nmp_update.fbs`).
//!
//! The schema (`crates/nmp-marmot/schema/marmot_snapshot.fbs`) mirrors the Rust
//! struct field-for-field. Every `Option<T>` carries a `has_x` presence flag plus
//! the value so absent (`None`) round-trips distinctly from a present-empty
//! string / zero value — the same optional-field convention used by
//! `content_tree.fbs` / `wallet_status.fbs` / `wot_bootstrap.fbs`.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input;
//! there are no `unwrap`/`expect`/panicking-index operations on the decode path.

// The generated FlatBuffers bindings are intrinsically `unsafe` (every accessor
// reads from a raw `Table`). This `allow` block scopes the relaxation to the
// single generated module — no hand-written code in this file uses `unsafe`.
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
#[path = "generated/marmot_snapshot_generated.rs"]
pub mod generated;

use generated::nmp::marmot as fb;

use crate::projection::payload::{
    KeyPackageStatus, LastOpError, MarmotGroupRow, MarmotSnapshot, PendingOpRow, PendingWelcomeRow,
};
use nmp_core::TypedProjectionData;

/// Host-declared projection key this typed payload is emitted under.
pub const PROJECTION_KEY: &str = "nmp.marmot.snapshot";
/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.marmot.snapshot";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NMMS";
/// Wire schema version. Bump on any additive or breaking change to `marmot_snapshot.fbs`.
/// v2: added `PendingOpRow` (with `age_secs`) + `LastOpError` tables and the
/// `pending_ops` / `last_op_error` fields on `MarmotSnapshot`.
/// v3: removed `age_display` / `subtitle` / `action_label` from `KeyPackageStatus`
/// (presentation formatting moved to shells per aim.md §2); added `is_registered`.
/// v4: removed presentation fields (`display_name` / `initials` from `MarmotGroupRow`;
/// `display_name` from `PendingWelcomeRow`; `display_label` from `PendingOpRow`;
/// `has_invites_chip_label` + `invites_chip_label` from `MarmotSnapshot`).
/// Shells now own all fallback copy, initials computation, and pluralisation.
pub const SCHEMA_VERSION: u32 = 4;

// --- typed-projection envelope -------------------------------------------

/// Build the [`TypedProjectionData`] sidecar entry for a snapshot — the value
/// `register_typed_snapshot_projection`'s closure returns and the kernel
/// collects into a frame's `typed_projections` sidecar.
#[must_use]
pub fn typed_projection(snapshot: &MarmotSnapshot) -> TypedProjectionData {
    TypedProjectionData {
        key: PROJECTION_KEY.to_string(),
        schema_id: SCHEMA_ID.to_string(),
        schema_version: SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(FILE_IDENTIFIER).into_owned(),
        payload: encode_marmot_snapshot(snapshot),
        ..Default::default()
    }
}

// --- encode ---------------------------------------------------------------

/// Encode a [`MarmotSnapshot`] to typed FlatBuffers bytes (with the `NMMS`
/// file identifier).
#[must_use]
pub fn encode_marmot_snapshot(snapshot: &MarmotSnapshot) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    // All child offsets must be created before each parent table is started.
    let groups: Vec<_> = snapshot
        .groups
        .iter()
        .map(|g| encode_group_row(&mut fbb, g))
        .collect();
    let groups = fbb.create_vector(&groups);

    let welcomes: Vec<_> = snapshot
        .pending_welcomes
        .iter()
        .map(|w| encode_welcome_row(&mut fbb, w))
        .collect();
    let pending_welcomes = fbb.create_vector(&welcomes);

    let key_package = encode_key_package(&mut fbb, &snapshot.key_package);

    let cached: Vec<_> = snapshot
        .cached_kp_pubkeys
        .iter()
        .map(|s| fbb.create_string(s))
        .collect();
    let cached_kp_pubkeys = fbb.create_vector(&cached);

    let pending_op_offsets: Vec<_> = snapshot
        .pending_ops
        .iter()
        .map(|op| encode_pending_op_row(&mut fbb, op))
        .collect();
    let pending_ops = fbb.create_vector(&pending_op_offsets);

    let last_op_error = snapshot
        .last_op_error
        .as_ref()
        .map(|e| encode_last_op_error(&mut fbb, e));

    let root = fb::MarmotSnapshot::create(
        &mut fbb,
        &fb::MarmotSnapshotArgs {
            schema_version: SCHEMA_VERSION,
            groups: Some(groups),
            pending_welcomes: Some(pending_welcomes),
            key_package: Some(key_package),
            cached_kp_pubkeys: Some(cached_kp_pubkeys),
            is_registered: snapshot.is_registered,
            orphaned_commit_count: snapshot.orphaned_commit_count,
            keyring_unavailable: snapshot.keyring_unavailable,
            pending_ops: Some(pending_ops),
            last_op_error,
        },
    );
    fb::finish_marmot_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

type Off<'a, T> = flatbuffers::WIPOffset<T>;

fn encode_pending_op_row<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    op: &PendingOpRow,
) -> Off<'a, fb::PendingOpRow<'a>> {
    let correlation_id = fbb.create_string(&op.correlation_id);
    let op_tag = fbb.create_string(&op.op_tag);
    fb::PendingOpRow::create(
        fbb,
        &fb::PendingOpRowArgs {
            correlation_id: Some(correlation_id),
            op_tag: Some(op_tag),
            missing_count: op.missing_count,
            age_secs: op.age_secs,
        },
    )
}

fn encode_last_op_error<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    e: &LastOpError,
) -> Off<'a, fb::LastOpError<'a>> {
    let op = fbb.create_string(&e.op);
    let reason = fbb.create_string(&e.reason);
    let correlation_id = fbb.create_string(&e.correlation_id);
    fb::LastOpError::create(
        fbb,
        &fb::LastOpErrorArgs {
            op: Some(op),
            reason: Some(reason),
            at_secs: e.at_secs,
            correlation_id: Some(correlation_id),
        },
    )
}

fn encode_group_row<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    g: &MarmotGroupRow,
) -> Off<'a, fb::MarmotGroupRow<'a>> {
    let id_hex = fbb.create_string(&g.id_hex);
    let name = fbb.create_string(&g.name);
    let members: Vec<_> = g.members.iter().map(|m| fbb.create_string(m)).collect();
    let members = fbb.create_vector(&members);
    fb::MarmotGroupRow::create(
        fbb,
        &fb::MarmotGroupRowArgs {
            id_hex: Some(id_hex),
            name: Some(name),
            members: Some(members),
            member_count: g.member_count,
            has_unread_count: g.unread_count.is_some(),
            unread_count: g.unread_count.unwrap_or(0),
            has_last_msg_at: g.last_msg_at.is_some(),
            last_msg_at: g.last_msg_at.unwrap_or(0),
        },
    )
}

fn encode_welcome_row<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    w: &PendingWelcomeRow,
) -> Off<'a, fb::PendingWelcomeRow<'a>> {
    let id_hex = fbb.create_string(&w.id_hex);
    let group_name = fbb.create_string(&w.group_name);
    let inviter_npub = fbb.create_string(&w.inviter_npub);
    fb::PendingWelcomeRow::create(
        fbb,
        &fb::PendingWelcomeRowArgs {
            id_hex: Some(id_hex),
            group_name: Some(group_name),
            inviter_npub: Some(inviter_npub),
        },
    )
}

fn encode_key_package<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    kp: &KeyPackageStatus,
) -> Off<'a, fb::KeyPackageStatus<'a>> {
    let d_tag = kp.d_tag.as_ref().map(|s| fbb.create_string(s));
    fb::KeyPackageStatus::create(
        fbb,
        &fb::KeyPackageStatusArgs {
            published: kp.published,
            has_d_tag: kp.d_tag.is_some(),
            d_tag,
            has_age_secs: kp.age_secs.is_some(),
            age_secs: kp.age_secs.unwrap_or(0),
            stale: kp.stale,
            is_registered: kp.is_registered,
        },
    )
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_marmot_snapshot`])
/// back into a [`MarmotSnapshot`]. Returns an error string on any malformed
/// input.
pub fn decode_marmot_snapshot(bytes: &[u8]) -> Result<MarmotSnapshot, String> {
    if bytes.len() < 8 || !fb::marmot_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NMMS file identifier".to_string());
    }
    let root = fb::root_as_marmot_snapshot(bytes)
        .map_err(|e| format!("not a valid MarmotSnapshot buffer: {e}"))?;

    let groups = root
        .groups()
        .map(|v| v.iter().map(decode_group_row).collect())
        .unwrap_or_default();
    let pending_welcomes = root
        .pending_welcomes()
        .map(|v| v.iter().map(decode_welcome_row).collect())
        .unwrap_or_default();
    let cached_kp_pubkeys = root
        .cached_kp_pubkeys()
        .map(|v| v.iter().map(str::to_string).collect())
        .unwrap_or_default();

    let pending_ops = root
        .pending_ops()
        .map(|v| v.iter().map(decode_pending_op_row).collect())
        .unwrap_or_default();

    Ok(MarmotSnapshot {
        groups,
        pending_welcomes,
        key_package: root.key_package().map(decode_key_package).unwrap_or_default(),
        cached_kp_pubkeys,
        is_registered: root.is_registered(),
        orphaned_commit_count: root.orphaned_commit_count(),
        keyring_unavailable: root.keyring_unavailable(),
        pending_ops,
        last_op_error: root.last_op_error().map(decode_last_op_error),
    })
}

fn decode_pending_op_row(op: fb::PendingOpRow<'_>) -> PendingOpRow {
    PendingOpRow {
        correlation_id: op.correlation_id().unwrap_or_default().to_string(),
        op_tag: op.op_tag().unwrap_or_default().to_string(),
        missing_count: op.missing_count(),
        age_secs: op.age_secs(),
    }
}

fn decode_last_op_error(e: fb::LastOpError<'_>) -> LastOpError {
    LastOpError {
        op: e.op().unwrap_or_default().to_string(),
        reason: e.reason().unwrap_or_default().to_string(),
        at_secs: e.at_secs(),
        correlation_id: e.correlation_id().unwrap_or_default().to_string(),
    }
}

fn decode_group_row(g: fb::MarmotGroupRow<'_>) -> MarmotGroupRow {
    MarmotGroupRow {
        id_hex: g.id_hex().unwrap_or_default().to_string(),
        name: g.name().unwrap_or_default().to_string(),
        members: g
            .members()
            .map(|v| v.iter().map(str::to_string).collect())
            .unwrap_or_default(),
        member_count: g.member_count(),
        unread_count: optional_u32(g.has_unread_count(), g.unread_count()),
        last_msg_at: optional_u64(g.has_last_msg_at(), g.last_msg_at()),
    }
}

fn decode_welcome_row(w: fb::PendingWelcomeRow<'_>) -> PendingWelcomeRow {
    PendingWelcomeRow {
        id_hex: w.id_hex().unwrap_or_default().to_string(),
        group_name: w.group_name().unwrap_or_default().to_string(),
        inviter_npub: w.inviter_npub().unwrap_or_default().to_string(),
    }
}

fn decode_key_package(kp: fb::KeyPackageStatus<'_>) -> KeyPackageStatus {
    KeyPackageStatus {
        published: kp.published(),
        d_tag: optional_string(kp.has_d_tag(), kp.d_tag()),
        age_secs: optional_u64(kp.has_age_secs(), kp.age_secs()),
        stale: kp.stale(),
        is_registered: kp.is_registered(),
    }
}

/// Reconstruct an `Option<String>` from a `has_*` flag + the wire string,
/// distinguishing absent (`None`) from present-empty (`Some("")`).
/// Used for `KeyPackageStatus.d_tag` (the only remaining optional string field).
fn optional_string(present: bool, value: Option<&str>) -> Option<String> {
    present.then(|| value.unwrap_or_default().to_string())
}

fn optional_u32(present: bool, value: u32) -> Option<u32> {
    present.then_some(value)
}

fn optional_u64(present: bool, value: u64) -> Option<u64> {
    present.then_some(value)
}

#[cfg(test)]
#[path = "snapshot_fb_tests.rs"]
mod tests;
