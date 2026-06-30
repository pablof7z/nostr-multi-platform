use flatbuffers::{FlatBufferBuilder, WIPOffset};
use nmp_threading::{ThreadPointer, TimelineBlock};

use super::{fb, SCHEMA_VERSION};
use crate::timeline_projection::ModularTimelineSnapshot;

#[must_use]
pub fn encode_modular_timeline_snapshot(snapshot: &ModularTimelineSnapshot) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();

    let blocks: Vec<WIPOffset<fb::TimelineBlockEntry<'_>>> = snapshot
        .blocks
        .iter()
        .map(|block| encode_block(&mut builder, block))
        .collect();
    let blocks = builder.create_vector(&blocks);

    let root = fb::ModularTimelineSnapshot::create(
        &mut builder,
        &fb::ModularTimelineSnapshotArgs {
            schema_version: SCHEMA_VERSION,
            blocks: Some(blocks),
        },
    );
    fb::finish_modular_timeline_snapshot_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

fn encode_thread_pointer<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    pointer: &ThreadPointer,
) -> WIPOffset<fb::ThreadPointer<'bldr>> {
    match pointer {
        ThreadPointer::Event { id, relay, kind } => {
            let id = builder.create_string(id);
            let relay = relay.as_ref().map(|r| builder.create_string(r));
            fb::ThreadPointer::create(
                builder,
                &fb::ThreadPointerArgs {
                    kind: fb::ThreadPointerKind::Event,
                    id: Some(id),
                    coord: None,
                    uri: None,
                    relay,
                    has_kind_num: kind.is_some(),
                    kind_num: kind.unwrap_or_default(),
                },
            )
        }
        ThreadPointer::Address { coord, relay, kind } => {
            let coord = builder.create_string(coord);
            let relay = relay.as_ref().map(|r| builder.create_string(r));
            fb::ThreadPointer::create(
                builder,
                &fb::ThreadPointerArgs {
                    kind: fb::ThreadPointerKind::Address,
                    id: None,
                    coord: Some(coord),
                    uri: None,
                    relay,
                    has_kind_num: kind.is_some(),
                    kind_num: kind.unwrap_or_default(),
                },
            )
        }
        ThreadPointer::External { uri } => {
            let uri = builder.create_string(uri);
            fb::ThreadPointer::create(
                builder,
                &fb::ThreadPointerArgs {
                    kind: fb::ThreadPointerKind::External,
                    id: None,
                    coord: None,
                    uri: Some(uri),
                    relay: None,
                    has_kind_num: false,
                    kind_num: 0,
                },
            )
        }
    }
}

fn encode_block<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    block: &TimelineBlock,
) -> WIPOffset<fb::TimelineBlockEntry<'bldr>> {
    match block {
        TimelineBlock::Standalone { id, root } => {
            let standalone_id = builder.create_string(id);
            let standalone_root = root.as_ref().map(|r| encode_thread_pointer(builder, r));
            fb::TimelineBlockEntry::create(
                builder,
                &fb::TimelineBlockEntryArgs {
                    kind: fb::TimelineBlockKind::Standalone,
                    standalone_id: Some(standalone_id),
                    standalone_root,
                    module_event_ids: None,
                    module_has_gap: false,
                    module_root: None,
                },
            )
        }
        TimelineBlock::Module {
            events,
            has_gap,
            root,
        } => {
            let module_root = root.as_ref().map(|r| encode_thread_pointer(builder, r));
            let entries: Vec<WIPOffset<fb::BlockEventId<'_>>> = events
                .iter()
                .map(|event_id| {
                    let id = builder.create_string(event_id);
                    fb::BlockEventId::create(builder, &fb::BlockEventIdArgs { id: Some(id) })
                })
                .collect();
            let module_event_ids = builder.create_vector(&entries);
            fb::TimelineBlockEntry::create(
                builder,
                &fb::TimelineBlockEntryArgs {
                    kind: fb::TimelineBlockKind::Module,
                    standalone_id: None,
                    standalone_root: None,
                    module_event_ids: Some(module_event_ids),
                    module_has_gap: *has_gap,
                    module_root,
                },
            )
        }
    }
}
