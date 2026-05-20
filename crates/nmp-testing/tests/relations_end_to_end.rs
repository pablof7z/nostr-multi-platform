//! End-to-end integration test for the relation surface this milestone
//! delivered — the four protocol crates (`nmp-nip01`, `nmp-nip22`,
//! `nmp-nip57`, `nmp-relations`) plus the cross-NIP `Relations` facade.
//!
//! Asserts the applesauce-shape ergonomic the user asked for: with a single
//! root event and one child event of each relevant kind, every per-kind
//! ViewModule lands the child in its payload, and the facade entrypoints
//! produce correctly-tagged `UnsignedEvent`s.
//!
//! No real relays, no signing pipeline — this drives the substrate seams
//! directly (`ViewModule::on_event_inserted` → `snapshot`) so the test is
//! deterministic and fast.

use nmp_core::substrate::{KernelEvent, ViewContext, ViewModule};
use nmp_nip01::{
    NoteRecord, RepliesPayload, RepliesSpec, RepliesView, ThreadPayload, ThreadSpec, ThreadView,
};
use nmp_nip22::{CommentsPayload, CommentsSpec, CommentsView};
use nmp_nip57::{ZapsPayload, ZapsSpec, ZapsView};
use nmp_relations::{
    decode::ReactionTarget,
    relations::{RelationSpecs, Relations},
    ReactionSummaryPayload, ReactionSummarySpec, ReactionSummaryView, RepostsPayload, RepostsSpec,
    RepostsView,
};

const ROOT_AUTHOR: &str = "aaaa";
const REPLIER: &str = "bbbb";
const REACTOR: &str = "cccc";
const REPOSTER: &str = "dddd";
const ZAPPER: &str = "eeee";
const COMMENTER: &str = "ffff";

fn root_event_id() -> &'static str {
    "ROOTID"
}

fn ke(id: &str, author: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>, content: &str) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: author.into(),
        kind,
        created_at,
        tags,
        content: content.into(),
    }
}

fn ctx() -> ViewContext {
    ViewContext::default()
}

fn root_note() -> KernelEvent {
    ke(root_event_id(), ROOT_AUTHOR, 1, 100, vec![], "hello world")
}

fn root_record() -> NoteRecord {
    nmp_nip01::try_from_kernel_event(&root_note()).expect("root is a valid kind-1")
}

fn reply_event() -> KernelEvent {
    // Marked-form NIP-10 reply to root.
    ke(
        "REPLYID",
        REPLIER,
        1,
        101,
        vec![
            vec!["e".into(), root_event_id().into(), "".into(), "root".into()],
            vec!["e".into(), root_event_id().into(), "".into(), "reply".into()],
            vec!["p".into(), ROOT_AUTHOR.into()],
        ],
        "great post",
    )
}

fn reaction_event() -> KernelEvent {
    ke(
        "REACTID",
        REACTOR,
        7,
        102,
        vec![
            vec!["e".into(), root_event_id().into()],
            vec!["p".into(), ROOT_AUTHOR.into()],
        ],
        "+",
    )
}

fn repost_event() -> KernelEvent {
    ke(
        "REPOSTID",
        REPOSTER,
        6,
        103,
        vec![
            vec!["e".into(), root_event_id().into()],
            vec!["p".into(), ROOT_AUTHOR.into()],
        ],
        "",
    )
}

fn zap_receipt_event() -> KernelEvent {
    // 150 sats = 150_000 msat (lnbc1500n HRP).
    ke(
        "ZAPID",
        "ln_node",
        9735,
        104,
        vec![
            vec!["p".into(), ROOT_AUTHOR.into()],
            vec!["e".into(), root_event_id().into()],
            vec!["bolt11".into(), "lnbc1500n1pvjluez000".into()],
            vec!["P".into(), ZAPPER.into()],
        ],
        "",
    )
}

fn comment_event() -> KernelEvent {
    ke(
        "COMMENTID",
        COMMENTER,
        1111,
        105,
        vec![
            vec!["E".into(), root_event_id().into()],
            vec!["K".into(), "1".into()],
            vec!["P".into(), ROOT_AUTHOR.into()],
            vec!["e".into(), root_event_id().into()],
            vec!["k".into(), "1".into()],
            vec!["p".into(), ROOT_AUTHOR.into()],
        ],
        "nice work",
    )
}

// ─── Per-view assertions ─────────────────────────────────────────────────────

#[test]
fn replies_view_lands_the_reply() {
    let spec = RepliesSpec { target: root_event_id().into() };
    let (mut state, _payload) = RepliesView::open(&ctx(), spec);
    RepliesView::on_event_inserted(&ctx(), &mut state, &reply_event());
    let snap: RepliesPayload = RepliesView::snapshot(&ctx(), &state);
    assert_eq!(snap.target_id, root_event_id());
    assert_eq!(snap.replies.len(), 1);
    assert_eq!(snap.replies[0].id, "REPLYID");
}

#[test]
fn thread_view_lands_root_and_reply_as_parent_child() {
    let spec = ThreadSpec { root_event: root_event_id().into() };
    let (mut state, _payload) = ThreadView::open(&ctx(), spec);
    ThreadView::on_event_inserted(&ctx(), &mut state, &root_note());
    ThreadView::on_event_inserted(&ctx(), &mut state, &reply_event());
    let snap: ThreadPayload = ThreadView::snapshot(&ctx(), &state);
    let ids: Vec<&str> = snap.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec![root_event_id(), "REPLYID"]);
    assert_eq!(snap.nodes[0].depth, 0);
    assert_eq!(snap.nodes[0].child_count, 1);
    assert_eq!(snap.nodes[1].depth, 1);
}

#[test]
fn reaction_summary_view_counts_the_reaction() {
    let spec = ReactionSummarySpec { target: ReactionTarget::Event(root_event_id().into()) };
    let (mut state, _) = ReactionSummaryView::open(&ctx(), spec);
    ReactionSummaryView::on_event_inserted(&ctx(), &mut state, &reaction_event());
    let snap: ReactionSummaryPayload = ReactionSummaryView::snapshot(&ctx(), &state);
    assert_eq!(snap.total, 1);
    assert!(snap.entries.iter().any(|(c, n)| c == "+" && *n == 1));
}

#[test]
fn reposts_view_surfaces_the_repost() {
    let spec = RepostsSpec::OfTarget(ReactionTarget::Event(root_event_id().into()));
    let (mut state, _) = RepostsView::open(&ctx(), spec);
    RepostsView::on_event_inserted(&ctx(), &mut state, &repost_event());
    let snap: RepostsPayload = RepostsView::snapshot(&ctx(), &state);
    assert_eq!(snap.reposts.len(), 1);
}

#[test]
fn zaps_view_totals_the_zap_msats() {
    let spec = ZapsSpec { target: root_event_id().into() };
    let (mut state, _) = ZapsView::open(&ctx(), spec);
    ZapsView::on_event_inserted(&ctx(), &mut state, &zap_receipt_event());
    let snap: ZapsPayload = ZapsView::snapshot(&ctx(), &state);
    assert_eq!(snap.target_id, root_event_id());
    assert_eq!(snap.zap_count, 1);
    assert_eq!(snap.total_msats, 150_000);
    assert_eq!(snap.zappers[0].pubkey.as_deref(), Some(ZAPPER));
}

#[test]
fn comments_view_lands_the_comment() {
    let spec = CommentsSpec { target: root_event_id().into() };
    let (mut state, _) = CommentsView::open(&ctx(), spec);
    CommentsView::on_event_inserted(&ctx(), &mut state, &comment_event());
    let snap: CommentsPayload = CommentsView::snapshot(&ctx(), &state);
    assert_eq!(snap.comments.len(), 1);
    assert_eq!(snap.comments[0].id, "COMMENTID");
}

// ─── Facade ──────────────────────────────────────────────────────────────────

#[test]
fn facade_for_event_wires_every_spec_to_the_root() {
    let specs: RelationSpecs = Relations::for_event(root_event_id(), 1);
    assert!(specs.replies.is_some());
    assert!(specs.thread.is_some());
    assert_eq!(specs.replies.as_ref().unwrap().target, root_event_id());
    assert_eq!(specs.thread.as_ref().unwrap().root_event, root_event_id());
    assert_eq!(specs.zaps.target, root_event_id());
    assert_eq!(specs.comments.target, root_event_id());
    match &specs.reactions.target {
        ReactionTarget::Event(id) => assert_eq!(id, root_event_id()),
        _ => panic!("expected Event reaction target"),
    }
    match &specs.reposts {
        RepostsSpec::OfTarget(ReactionTarget::Event(id)) => assert_eq!(id, root_event_id()),
        _ => panic!("expected OfTarget(Event(_))"),
    }
}

#[test]
fn facade_for_event_drives_views_end_to_end() {
    // The whole point of the facade: hand the specs to `ViewModule::open` and
    // drive each with the same child events used above — every payload is
    // populated without the caller writing any per-NIP wiring.
    let specs = Relations::for_event(root_event_id(), 1);

    let (mut r_state, _) = RepliesView::open(&ctx(), specs.replies.unwrap());
    RepliesView::on_event_inserted(&ctx(), &mut r_state, &reply_event());
    assert_eq!(RepliesView::snapshot(&ctx(), &r_state).replies.len(), 1);

    let (mut t_state, _) = ThreadView::open(&ctx(), specs.thread.unwrap());
    ThreadView::on_event_inserted(&ctx(), &mut t_state, &root_note());
    ThreadView::on_event_inserted(&ctx(), &mut t_state, &reply_event());
    assert_eq!(ThreadView::snapshot(&ctx(), &t_state).nodes.len(), 2);

    let (mut rx_state, _) = ReactionSummaryView::open(&ctx(), specs.reactions);
    ReactionSummaryView::on_event_inserted(&ctx(), &mut rx_state, &reaction_event());
    assert_eq!(ReactionSummaryView::snapshot(&ctx(), &rx_state).total, 1);

    let (mut rp_state, _) = RepostsView::open(&ctx(), specs.reposts);
    RepostsView::on_event_inserted(&ctx(), &mut rp_state, &repost_event());
    assert_eq!(RepostsView::snapshot(&ctx(), &rp_state).reposts.len(), 1);

    let (mut z_state, _) = ZapsView::open(&ctx(), specs.zaps);
    ZapsView::on_event_inserted(&ctx(), &mut z_state, &zap_receipt_event());
    assert_eq!(ZapsView::snapshot(&ctx(), &z_state).total_msats, 150_000);

    let (mut c_state, _) = CommentsView::open(&ctx(), specs.comments);
    CommentsView::on_event_inserted(&ctx(), &mut c_state, &comment_event());
    assert_eq!(CommentsView::snapshot(&ctx(), &c_state).comments.len(), 1);
}

#[test]
fn facade_reply_to_produces_a_correct_nip10_reply() {
    let parent = root_record();
    let unsigned = Relations::reply_to(&parent, "great").build(REPLIER, 200).unwrap();
    let keys: Vec<&str> = unsigned.tags.iter().filter_map(|t| t.first()).map(String::as_str).collect();
    assert_eq!(keys, vec!["e", "e", "p"]);
    assert_eq!(unsigned.tags[0][1], root_event_id()); // root marker
    assert_eq!(unsigned.tags[0][3], "root");
    assert_eq!(unsigned.tags[1][1], root_event_id()); // reply marker
    assert_eq!(unsigned.tags[1][3], "reply");
    assert_eq!(unsigned.tags[2][1], ROOT_AUTHOR);
    assert_eq!(unsigned.kind, 1);
}

#[test]
fn facade_react_to_produces_a_correct_kind_7() {
    let parent = root_record();
    let unsigned = Relations::react_to(&parent).build(REACTOR, 201).unwrap();
    assert_eq!(unsigned.kind, 7);
    assert_eq!(unsigned.tags[0][0], "e");
    assert_eq!(unsigned.tags[0][1], root_event_id());
}

#[test]
fn facade_repost_produces_a_correct_kind_6() {
    let parent = root_record();
    let unsigned = Relations::repost(&parent).build(REPOSTER, 202).unwrap();
    assert_eq!(unsigned.kind, 6);
    assert_eq!(unsigned.tags[0][1], root_event_id());
}

#[test]
fn facade_zap_request_pre_wires_recipient_and_event() {
    let parent = root_record();
    let unsigned = Relations::zap_request(&parent)
        .amount_msats(21_000)
        .relays(vec!["wss://r.x".into()])
        .build(ZAPPER, 203)
        .unwrap();
    assert_eq!(unsigned.kind, 9734);
    let p = unsigned.tags.iter().find(|t| t[0] == "p").unwrap();
    assert_eq!(p[1], ROOT_AUTHOR);
    let e = unsigned.tags.iter().find(|t| t[0] == "e").unwrap();
    assert_eq!(e[1], root_event_id());
}

#[test]
fn facade_comment_on_produces_a_correct_kind_1111() {
    let parent = root_record();
    let unsigned = Relations::comment_on(&parent)
        .content("nice")
        .build(COMMENTER, 204)
        .unwrap();
    assert_eq!(unsigned.kind, 1111);
    let upper_e = unsigned.tags.iter().find(|t| t[0] == "E").unwrap();
    assert_eq!(upper_e[1], root_event_id());
    let upper_k = unsigned.tags.iter().find(|t| t[0] == "K").unwrap();
    assert_eq!(upper_k[1], "1");
    let upper_p = unsigned.tags.iter().find(|t| t[0] == "P").unwrap();
    assert_eq!(upper_p[1], ROOT_AUTHOR);
}
