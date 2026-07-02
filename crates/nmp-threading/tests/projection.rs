use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_threading::{
    decode_threading_snapshot, threading_projection_key, ModulePolicy, ThreadPointer,
    ThreadingProjection, TimelineBlock, THREADING_GRAPH_SESSION_ID_MAX_LEN,
};

fn tag(cells: &[&str]) -> Vec<String> {
    cells.iter().map(|cell| (*cell).to_string()).collect()
}

fn event(id: &str, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: format!("author-{id}"),
        kind: 9,
        created_at,
        tags,
        content: format!("content {id}"),
        relay_provenance: Vec::new(),
    }
}

fn reply(id: &str, created_at: u64, root: &str, parent: &str) -> KernelEvent {
    event(
        id,
        created_at,
        vec![
            tag(&["e", root, "", "root"]),
            tag(&["e", parent, "", "reply"]),
            tag(&["p", "parent-author"]),
        ],
    )
}

#[test]
fn projection_emits_edges_blocks_and_typed_wire() {
    let projection = ThreadingProjection::etag(ModulePolicy::default());
    projection.on_kernel_event(&event("root", 1, vec![]));
    projection.on_kernel_event(&reply("child", 2, "root", "root"));

    let snapshot = projection.snapshot();
    assert_eq!(snapshot.edges.len(), 2);
    let child = snapshot
        .edges
        .iter()
        .find(|edge| edge.event_id == "child")
        .expect("child edge");
    assert_eq!(child.parent_author_pubkey.as_deref(), Some("parent-author"));
    assert_eq!(
        child.parent,
        Some(ThreadPointer::Event {
            id: "root".to_string(),
            relay: None,
            kind: None,
        })
    );
    assert_eq!(
        snapshot.blocks,
        vec![TimelineBlock::Module {
            events: vec!["root".to_string(), "child".to_string()],
            has_gap: false,
            root: Some(ThreadPointer::Event {
                id: "root".to_string(),
                relay: None,
                kind: None,
            }),
        }]
    );

    let typed = projection.typed_projection("nmp.threading.graph.chat");
    assert_eq!(typed.key, "nmp.threading.graph.chat");
    assert_eq!(typed.schema_id, "nmp.threading.graph");
    assert_eq!(typed.schema_version, 1);
    assert_eq!(typed.file_identifier, "NTHR");
    assert_eq!(
        decode_threading_snapshot(&typed.payload).expect("decode"),
        snapshot
    );
}

#[test]
fn orphan_reply_hydrates_when_parent_arrives() {
    let projection = ThreadingProjection::etag(ModulePolicy::default());
    projection.on_kernel_event(&reply("child", 2, "root", "root"));

    let before = projection.snapshot();
    assert!(before.blocks.is_empty());
    assert_eq!(before.pending_ancestor_ids, vec!["root"]);

    projection.on_kernel_event(&event("root", 1, vec![]));
    let after = projection.snapshot();
    assert!(after.pending_ancestor_ids.is_empty());
    assert_eq!(
        after.blocks,
        vec![TimelineBlock::Module {
            events: vec!["root".to_string(), "child".to_string()],
            has_gap: false,
            root: Some(ThreadPointer::Event {
                id: "root".to_string(),
                relay: None,
                kind: None,
            }),
        }]
    );
}

#[test]
fn projection_key_accepts_only_bounded_session_suffixes() {
    assert_eq!(
        threading_projection_key("group.chat-1").as_deref(),
        Some("nmp.threading.graph.group.chat-1")
    );
    assert!(threading_projection_key("").is_none());
    assert!(threading_projection_key("bad key").is_none());
    assert!(threading_projection_key("bad/key").is_none());
    assert!(
        threading_projection_key(&"x".repeat(THREADING_GRAPH_SESSION_ID_MAX_LEN + 1)).is_none()
    );
}
