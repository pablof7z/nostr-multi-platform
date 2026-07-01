//! Typed FlatBuffers codec for browser notification snapshots.

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
#[path = "generated/notifications_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use generated::nmp::notifications as fb;

use super::super::projection::{
    NotificationRow, NotificationsSnapshot, NOTIFICATIONS_FILE_IDENTIFIER,
    NOTIFICATIONS_SCHEMA_VERSION,
};

#[must_use]
pub fn encode_notifications_snapshot(snapshot: &NotificationsSnapshot) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let rows: Vec<_> = snapshot
        .rows
        .iter()
        .map(|row| encode_row(&mut fbb, row))
        .collect();
    let rows = fbb.create_vector(&rows);
    let viewer_pubkey = fbb.create_string(&snapshot.viewer_pubkey);
    let root = fb::NotificationsSnapshot::create(
        &mut fbb,
        &fb::NotificationsSnapshotArgs {
            schema_version: NOTIFICATIONS_SCHEMA_VERSION,
            viewer_pubkey: Some(viewer_pubkey),
            rows: Some(rows),
            unread_count: snapshot.unread_count,
        },
    );
    fb::finish_notifications_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_row<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &NotificationRow,
) -> WIPOffset<fb::NotificationRow<'a>> {
    let event_id = fbb.create_string(&row.event_id);
    let actor_pubkey = fbb.create_string(&row.actor_pubkey);
    let notification_kind = fbb.create_string(row.notification_kind.as_str());
    let content = fbb.create_string(&row.content);
    let target_event_id = row.target_event_id.as_ref().map(|id| fbb.create_string(id));
    let source_relays = if row.source_relays.is_empty() {
        None
    } else {
        let offsets: Vec<_> = row
            .source_relays
            .iter()
            .map(|relay| fbb.create_string(relay))
            .collect();
        Some(fbb.create_vector(&offsets))
    };
    fb::NotificationRow::create(
        fbb,
        &fb::NotificationRowArgs {
            event_id: Some(event_id),
            actor_pubkey: Some(actor_pubkey),
            event_kind: row.event_kind,
            notification_kind: Some(notification_kind),
            created_at: row.created_at,
            content: Some(content),
            target_event_id,
            source_relays,
            read: row.read,
        },
    )
}

#[must_use]
pub fn notifications_file_identifier() -> String {
    String::from_utf8_lossy(NOTIFICATIONS_FILE_IDENTIFIER).into_owned()
}
