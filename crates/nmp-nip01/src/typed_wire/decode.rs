use nmp_threading::{ThreadPointer, TimelineBlock};

use super::fb;
use crate::timeline_projection::ModularTimelineSnapshot;

pub fn decode_modular_timeline_snapshot(bytes: &[u8]) -> Result<ModularTimelineSnapshot, String> {
    if bytes.len() < 8 || !fb::modular_timeline_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NFTS file identifier".to_string());
    }
    let snapshot =
        fb::root_as_modular_timeline_snapshot(bytes).map_err(|err| format!("{err:?}"))?;

    let mut blocks = Vec::new();
    if let Some(fb_blocks) = snapshot.blocks() {
        blocks.reserve(fb_blocks.len());
        for index in 0..fb_blocks.len() {
            blocks.push(decode_block(fb_blocks.get(index))?);
        }
    }

    Ok(ModularTimelineSnapshot { blocks })
}

fn decode_thread_pointer(pointer: fb::ThreadPointer<'_>) -> Result<ThreadPointer, String> {
    let kind = optional_kind_num(pointer.has_kind_num(), pointer.kind_num());
    let relay = pointer.relay().map(str::to_string);
    match pointer.kind() {
        fb::ThreadPointerKind::Event => Ok(ThreadPointer::Event {
            id: pointer
                .id()
                .ok_or("Event ThreadPointer missing id")?
                .to_string(),
            relay,
            kind,
        }),
        fb::ThreadPointerKind::Address => Ok(ThreadPointer::Address {
            coord: pointer
                .coord()
                .ok_or("Address ThreadPointer missing coord")?
                .to_string(),
            relay,
            kind,
        }),
        fb::ThreadPointerKind::External => Ok(ThreadPointer::External {
            uri: pointer
                .uri()
                .ok_or("External ThreadPointer missing uri")?
                .to_string(),
        }),
        other => Err(format!("unknown ThreadPointerKind: {other:?}")),
    }
}

fn decode_block(block: fb::TimelineBlockEntry<'_>) -> Result<TimelineBlock, String> {
    match block.kind() {
        fb::TimelineBlockKind::Standalone => {
            let id = block
                .standalone_id()
                .ok_or("Standalone block missing standalone_id")?
                .to_string();
            let root = block
                .standalone_root()
                .map(decode_thread_pointer)
                .transpose()?;
            Ok(TimelineBlock::Standalone { id, root })
        }
        fb::TimelineBlockKind::Module => {
            let mut events = Vec::new();
            if let Some(ids) = block.module_event_ids() {
                events.reserve(ids.len());
                for index in 0..ids.len() {
                    events.push(
                        ids.get(index)
                            .id()
                            .ok_or("Module block event id missing")?
                            .to_string(),
                    );
                }
            }
            let root = block.module_root().map(decode_thread_pointer).transpose()?;
            Ok(TimelineBlock::Module {
                events,
                has_gap: block.module_has_gap(),
                root,
            })
        }
        other => Err(format!("unknown TimelineBlockKind: {other:?}")),
    }
}

fn optional_kind_num(present: bool, value: u32) -> Option<u32> {
    present.then_some(value)
}
