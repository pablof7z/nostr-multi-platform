//! Typed FlatBuffers wire codec for [`crate::projection::DiscoveredGroupsSnapshot`].
//!
//! The canonical FFI shape is the serde JSON of `DiscoveredGroupsSnapshot`
//! (`DiscoveredGroupsProjection::snapshot_json`). This module adds a **typed
//! FlatBuffers** encoding of the same read model — the typed sidecar (ADR-0037)
//! carried alongside the generic `Value` projection under the same
//! `"nmp.nip29.discovered_groups"` key. The serde shape stays authoritative; this
//! is the typed payload a `NDGS`-aware host decodes with generated accessors
//! instead of JSON reflection.
//!
//! Raw data only (ADR-0032): protocol fields are raw values. Presentation-layer
//! formatting (display-name fallback, avatar initials, subtitle) is the shell's
//! responsibility and is not encoded here.
//!
//! Optional fields: `name` / `picture` / `about` are `Option<String>` in
//! [`DiscoveredGroup`]. They are encoded as bare FlatBuffers `string` (absent ==
//! `None`) — the content_tree.fbs pattern. There is no present-empty vs absent
//! distinction to preserve for these tag-derived fields, so no `has_*` companion
//! is needed.
//!
//! Honours D6 (no panics): [`decode_discovered_groups_snapshot`] returns
//! `Err(String)` on any malformed input; there are no `unwrap`/`expect`/panicking
//! operations on the decode path.
//!
//! ## Regenerating the bindings
//!
//! The checked-in bindings in `wire/generated/discovered_groups_generated.rs` are
//! produced by `flatc` from `schema/discovered_groups.fbs`. Regenerate only with
//! the workspace FlatBuffers pin (`25.12.19`), enforced by
//! `ci/check-flatbuffers-version-pins.sh`. The schema is self-contained, so
//! generate with plain `flatc --rust`:
//!
//! ```sh
//! flatc --rust -o crates/nmp-nip29/src/wire/generated \
//!       crates/nmp-nip29/schema/discovered_groups.fbs
//! ```

// The generated FlatBuffers bindings are intrinsically `unsafe` (every accessor
// reads from a raw `Table`). This single generated module — and only it — opts
// back into `unsafe`. No hand-written code in this file uses `unsafe`.
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
#[path = "generated/discovered_groups_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use generated::nmp::nip_29 as fb;

use crate::projection::{DiscoveredGroup, DiscoveredGroupsSnapshot};

/// Stable schema identifier carried in the typed-projection envelope.
pub const DISCOVERED_GROUPS_SCHEMA_ID: &str = "nmp.nip29.discovered_groups";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const DISCOVERED_GROUPS_FILE_IDENTIFIER: &[u8; 4] = b"NDGS";
/// Wire schema version. Bump on any breaking change to `discovered_groups.fbs`.
pub const DISCOVERED_GROUPS_SCHEMA_VERSION: u32 = 1;

// --- encode ---------------------------------------------------------------

/// Encode a [`DiscoveredGroupsSnapshot`] to typed FlatBuffers bytes (with the
/// `NDGS` file identifier). `groups` order is preserved verbatim (alphabetical by
/// `group_id` as the projection emits it).
#[must_use]
pub fn encode_discovered_groups_snapshot(snapshot: &DiscoveredGroupsSnapshot) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    let group_offsets: Vec<WIPOffset<fb::DiscoveredGroup<'_>>> = snapshot
        .groups
        .iter()
        .map(|group| encode_group(&mut fbb, group))
        .collect();
    let groups = fbb.create_vector(&group_offsets);
    let host_relay_url = fbb.create_string(&snapshot.host_relay_url);

    let root = fb::DiscoveredGroupsSnapshot::create(
        &mut fbb,
        &fb::DiscoveredGroupsSnapshotArgs {
            schema_version: DISCOVERED_GROUPS_SCHEMA_VERSION,
            host_relay_url: Some(host_relay_url),
            groups: Some(groups),
        },
    );
    fb::finish_discovered_groups_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_group<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    group: &DiscoveredGroup,
) -> WIPOffset<fb::DiscoveredGroup<'a>> {
    let group_id = fbb.create_string(&group.group_id);
    let host_relay_url = fbb.create_string(&group.host_relay_url);
    // `Option<String>` → bare FlatBuffers `string`: `None` omits the field
    // (absent), `Some` writes the value. No has_* flag (absent == None).
    let name = group.name.as_ref().map(|s| fbb.create_string(s));
    let picture = group.picture.as_ref().map(|s| fbb.create_string(s));
    let about = group.about.as_ref().map(|s| fbb.create_string(s));

    fb::DiscoveredGroup::create(
        fbb,
        &fb::DiscoveredGroupArgs {
            group_id: Some(group_id),
            host_relay_url: Some(host_relay_url),
            name,
            picture,
            about,
            member_count: group.member_count,
            admin_count: group.admin_count,
            public: group.public,
            open: group.open,
        },
    )
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by
/// [`encode_discovered_groups_snapshot`]) back into a
/// [`DiscoveredGroupsSnapshot`]. Returns an error string on any malformed input
/// or missing required field.
pub fn decode_discovered_groups_snapshot(bytes: &[u8]) -> Result<DiscoveredGroupsSnapshot, String> {
    if bytes.len() < 8 || !fb::discovered_groups_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NDGS file identifier".to_string());
    }
    let root = fb::root_as_discovered_groups_snapshot(bytes)
        .map_err(|e| format!("not a valid DiscoveredGroupsSnapshot buffer: {e}"))?;

    let host_relay_url = str_field(
        root.host_relay_url(),
        "DiscoveredGroupsSnapshot.host_relay_url",
    )?;

    let mut groups = Vec::new();
    if let Some(fb_groups) = root.groups() {
        groups.reserve(fb_groups.len());
        for fb_group in fb_groups.iter() {
            groups.push(decode_group(fb_group)?);
        }
    }

    Ok(DiscoveredGroupsSnapshot {
        host_relay_url,
        groups,
    })
}

fn decode_group(group: fb::DiscoveredGroup<'_>) -> Result<DiscoveredGroup, String> {
    Ok(DiscoveredGroup {
        group_id: str_field(group.group_id(), "DiscoveredGroup.group_id")?,
        host_relay_url: str_field(group.host_relay_url(), "DiscoveredGroup.host_relay_url")?,
        name: group.name().map(str::to_string),
        picture: group.picture().map(str::to_string),
        about: group.about().map(str::to_string),
        member_count: group.member_count(),
        admin_count: group.admin_count(),
        public: group.public(),
        open: group.open(),
    })
}

/// Require a present string field; an absent FlatBuffers string on a mandatory
/// slot is a decode error.
fn str_field(value: Option<&str>, ctx: &str) -> Result<String, String> {
    value
        .map(str::to_string)
        .ok_or_else(|| format!("{ctx}: missing required string field"))
}
