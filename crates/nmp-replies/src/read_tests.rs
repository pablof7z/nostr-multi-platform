use super::*;
use crate::ReplyTarget;

const ROOT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const REPLY: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn event(id: &str, kind: u32, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: AUTHOR.to_string(),
        kind,
        created_at: 1,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: "body".to_string(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn note_read_plan_uses_kind1_e_tag_and_direct_acceptance() {
    let target = ReplyTarget::event(ROOT, KIND_SHORT_TEXT_NOTE, Some(AUTHOR.to_string())).unwrap();
    let plan = ReplyReadPlan::direct(target).unwrap();

    assert_eq!(plan.protocol, ReplyProtocol::Nip10);
    assert_eq!(plan.dependencies.kinds, vec![KIND_SHORT_TEXT_NOTE]);
    assert_eq!(
        plan.dependencies.tag_refs,
        vec![("e".to_string(), ROOT.to_string())]
    );
    assert_eq!(
        plan.filter_json(),
        format!(r##"{{"#e":["{ROOT}"],"kinds":[1]}}"##)
    );
    assert!(plan.accepts(&event(
        REPLY,
        KIND_SHORT_TEXT_NOTE,
        vec![vec!["e", ROOT, "", "reply"]]
    )));
    assert!(!plan.accepts(&event(
        "3333333333333333333333333333333333333333333333333333333333333333",
        KIND_SHORT_TEXT_NOTE,
        vec![vec!["e", "other", "", "reply"]]
    )));
}

#[test]
fn non_note_event_read_plan_uses_nip22_root_tag() {
    let target = ReplyTarget::event(ROOT, 30023, Some(AUTHOR.to_string())).unwrap();
    let plan = ReplyReadPlan::direct(target).unwrap();

    assert_eq!(plan.protocol, ReplyProtocol::Nip22);
    assert_eq!(plan.dependencies.kinds, vec![KIND_NIP22_COMMENT]);
    assert_eq!(
        plan.dependencies.tag_refs,
        vec![("E".to_string(), ROOT.to_string())]
    );
    assert!(plan.accepts(&event(
        REPLY,
        KIND_NIP22_COMMENT,
        vec![
            vec!["E", ROOT],
            vec!["K", "30023"],
            vec!["e", ROOT],
            vec!["k", "30023"]
        ]
    )));
}

#[test]
fn address_read_plan_uses_uppercase_root_query_but_lowercase_direct_acceptance() {
    let target = ReplyTarget::address("30023:pubkey:essay", 30023, None).unwrap();
    let plan = ReplyReadPlan::direct(target).unwrap();

    assert_eq!(
        plan.dependencies.tag_refs,
        vec![("A".to_string(), "30023:pubkey:essay".to_string())]
    );
    assert!(plan.accepts(&event(
        REPLY,
        KIND_NIP22_COMMENT,
        vec![
            vec!["A", "30023:pubkey:essay"],
            vec!["K", "30023"],
            vec!["a", "30023:pubkey:essay"],
            vec!["k", "30023"]
        ]
    )));
}
