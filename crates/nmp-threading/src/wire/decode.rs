use crate::{ThreadEdge, ThreadPointer, ThreadingSnapshot, TimelineBlock};

use super::fb;

pub fn decode_threading_snapshot(bytes: &[u8]) -> Result<ThreadingSnapshot, String> {
    if bytes.len() < 8 || !fb::threading_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NTHR file identifier".to_string());
    }
    let snapshot = fb::root_as_threading_snapshot(bytes)
        .map_err(|err| format!("not a valid ThreadingSnapshot buffer: {err}"))?;
    Ok(ThreadingSnapshot {
        edges: decode_edges(snapshot.edges())?,
        blocks: decode_blocks(snapshot.blocks())?,
        pending_ancestor_ids: decode_ids(snapshot.pending_ancestor_ids())?,
    })
}

fn decode_edges(
    edges: Option<flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<fb::ThreadEdge<'_>>>>,
) -> Result<Vec<ThreadEdge>, String> {
    let mut out = Vec::new();
    if let Some(edges) = edges {
        out.reserve(edges.len());
        for edge in edges.iter() {
            out.push(decode_edge(edge)?);
        }
    }
    Ok(out)
}

fn decode_edge(edge: fb::ThreadEdge<'_>) -> Result<ThreadEdge, String> {
    Ok(ThreadEdge {
        event_id: str_field(edge.event_id(), "ThreadEdge.event_id")?,
        author_pubkey: str_field(edge.author_pubkey(), "ThreadEdge.author_pubkey")?,
        kind: edge.kind(),
        created_at: edge.created_at(),
        parent: edge.parent().map(decode_pointer).transpose()?,
        root: edge.root().map(decode_pointer).transpose()?,
        parent_author_pubkey: edge.parent_author_pubkey().map(str::to_string),
    })
}

fn decode_blocks(
    blocks: Option<
        flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<fb::TimelineBlockEntry<'_>>>,
    >,
) -> Result<Vec<TimelineBlock>, String> {
    let mut out = Vec::new();
    if let Some(blocks) = blocks {
        out.reserve(blocks.len());
        for block in blocks.iter() {
            out.push(decode_block(block)?);
        }
    }
    Ok(out)
}

fn decode_block(block: fb::TimelineBlockEntry<'_>) -> Result<TimelineBlock, String> {
    match block.kind() {
        fb::TimelineBlockKind::Standalone => Ok(TimelineBlock::Standalone {
            id: str_field(block.standalone_id(), "TimelineBlockEntry.standalone_id")?,
            root: block.standalone_root().map(decode_pointer).transpose()?,
        }),
        fb::TimelineBlockKind::Module => Ok(TimelineBlock::Module {
            events: decode_ids(block.module_event_ids())?,
            has_gap: block.module_has_gap(),
            root: block.module_root().map(decode_pointer).transpose()?,
        }),
        other => Err(format!("unknown TimelineBlockKind: {other:?}")),
    }
}

fn decode_pointer(pointer: fb::ThreadPointer<'_>) -> Result<ThreadPointer, String> {
    let kind = optional_kind_num(pointer.has_kind_num(), pointer.kind_num());
    let relay = pointer.relay().map(str::to_string);
    match pointer.kind() {
        fb::ThreadPointerKind::Event => Ok(ThreadPointer::Event {
            id: str_field(pointer.id(), "ThreadPointer.id")?,
            relay,
            kind,
        }),
        fb::ThreadPointerKind::Address => Ok(ThreadPointer::Address {
            coord: str_field(pointer.coord(), "ThreadPointer.coord")?,
            relay,
            kind,
        }),
        fb::ThreadPointerKind::External => Ok(ThreadPointer::External {
            uri: str_field(pointer.uri(), "ThreadPointer.uri")?,
        }),
        other => Err(format!("unknown ThreadPointerKind: {other:?}")),
    }
}

fn decode_ids(
    ids: Option<flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<fb::BlockEventId<'_>>>>,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    if let Some(ids) = ids {
        out.reserve(ids.len());
        for id in ids.iter() {
            out.push(str_field(id.id(), "BlockEventId.id")?);
        }
    }
    Ok(out)
}

fn str_field(value: Option<&str>, ctx: &str) -> Result<String, String> {
    value
        .map(str::to_string)
        .ok_or_else(|| format!("{ctx}: missing required string field"))
}

const fn optional_kind_num(present: bool, value: u32) -> Option<u32> {
    if present {
        Some(value)
    } else {
        None
    }
}
