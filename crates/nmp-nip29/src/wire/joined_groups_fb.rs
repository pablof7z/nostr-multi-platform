//! Typed FlatBuffers wire codec for [`crate::projection::JoinedGroupsSnapshot`].

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
#[path = "generated/joined_groups_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use generated::nmp::nip_29 as fb;

use crate::projection::{JoinedGroup, JoinedGroupsSnapshot};

pub const JOINED_GROUPS_SCHEMA_ID: &str = "nmp.nip29.joined_groups";
pub const JOINED_GROUPS_FILE_IDENTIFIER: &[u8; 4] = b"NJGS";
pub const JOINED_GROUPS_SCHEMA_VERSION: u32 = 1;

#[must_use]
pub fn encode_joined_groups_snapshot(snapshot: &JoinedGroupsSnapshot) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    let group_offsets: Vec<WIPOffset<fb::JoinedGroup<'_>>> = snapshot
        .groups
        .iter()
        .map(|group| encode_group(&mut fbb, group))
        .collect();
    let groups = fbb.create_vector(&group_offsets);
    let active_pubkey = fbb.create_string(&snapshot.active_pubkey);

    let root = fb::JoinedGroupsSnapshot::create(
        &mut fbb,
        &fb::JoinedGroupsSnapshotArgs {
            schema_version: JOINED_GROUPS_SCHEMA_VERSION,
            active_pubkey: Some(active_pubkey),
            groups: Some(groups),
        },
    );
    fb::finish_joined_groups_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_group<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    group: &JoinedGroup,
) -> WIPOffset<fb::JoinedGroup<'a>> {
    let group_id = fbb.create_string(&group.group_id);
    let host_relay_url = fbb.create_string(&group.host_relay_url);
    let name = group.name.as_ref().map(|s| fbb.create_string(s));
    let picture = group.picture.as_ref().map(|s| fbb.create_string(s));
    let about = group.about.as_ref().map(|s| fbb.create_string(s));

    fb::JoinedGroup::create(
        fbb,
        &fb::JoinedGroupArgs {
            group_id: Some(group_id),
            host_relay_url: Some(host_relay_url),
            name,
            picture,
            about,
            member_count: group.member_count,
            admin_count: group.admin_count,
            public: group.public,
            open: group.open,
            is_member: group.is_member,
            is_admin: group.is_admin,
        },
    )
}

pub fn decode_joined_groups_snapshot(bytes: &[u8]) -> Result<JoinedGroupsSnapshot, String> {
    if bytes.len() < 8 || !fb::joined_groups_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NJGS file identifier".to_string());
    }
    let root = fb::root_as_joined_groups_snapshot(bytes)
        .map_err(|e| format!("not a valid JoinedGroupsSnapshot buffer: {e}"))?;

    let active_pubkey = str_field(root.active_pubkey(), "JoinedGroupsSnapshot.active_pubkey")?;
    let mut groups = Vec::new();
    if let Some(fb_groups) = root.groups() {
        groups.reserve(fb_groups.len());
        for fb_group in fb_groups.iter() {
            groups.push(decode_group(fb_group)?);
        }
    }

    Ok(JoinedGroupsSnapshot {
        active_pubkey,
        groups,
    })
}

fn decode_group(group: fb::JoinedGroup<'_>) -> Result<JoinedGroup, String> {
    Ok(JoinedGroup {
        group_id: str_field(group.group_id(), "JoinedGroup.group_id")?,
        host_relay_url: str_field(group.host_relay_url(), "JoinedGroup.host_relay_url")?,
        name: group.name().map(str::to_string),
        picture: group.picture().map(str::to_string),
        about: group.about().map(str::to_string),
        member_count: group.member_count(),
        admin_count: group.admin_count(),
        public: group.public(),
        open: group.open(),
        is_member: group.is_member(),
        is_admin: group.is_admin(),
    })
}

fn str_field(value: Option<&str>, ctx: &str) -> Result<String, String> {
    value
        .map(str::to_string)
        .ok_or_else(|| format!("{ctx}: missing required string field"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joined_groups_round_trips() {
        let snapshot = JoinedGroupsSnapshot {
            active_pubkey: "a".repeat(64),
            groups: vec![JoinedGroup {
                group_id: "room".to_string(),
                host_relay_url: "wss://groups.example.com".to_string(),
                name: Some("Room".to_string()),
                member_count: 3,
                admin_count: 1,
                public: true,
                open: false,
                is_member: true,
                is_admin: false,
                ..Default::default()
            }],
        };

        let bytes = encode_joined_groups_snapshot(&snapshot);
        let decoded = decode_joined_groups_snapshot(&bytes).expect("NJGS decodes");
        assert_eq!(decoded, snapshot);
    }
}
