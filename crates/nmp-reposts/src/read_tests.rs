use super::*;
use nmp_nip18::KIND_GENERIC_REPOST as GENERIC_REPOST_KIND;
use nmp_nip18::KIND_REPOST as REPOST_KIND;

fn plan_for(target_id: &str) -> RepostReadPlan {
    RepostReadPlan::new(&RepostTarget::note(target_id).unwrap())
}

fn event(id: &str, author: &str, kind: u32, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at: 1,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn filter_json_requests_repost_wrapper_and_delete_kinds_by_target_e_tag() {
    let target = "a".repeat(64);
    let plan = plan_for(&target);
    let value: Value = serde_json::from_str(&plan.filter_json()).unwrap();
    assert_eq!(
        value["kinds"],
        serde_json::json!([KIND_REPOST, KIND_GENERIC_REPOST, KIND_DELETE])
    );
    assert_eq!(value["#e"], serde_json::json!([target]));
}

#[test]
fn accepts_kind_6_repost_of_target() {
    let target = "b".repeat(64);
    let plan = plan_for(&target);
    let repost = event("r1", "alice", REPOST_KIND, vec![vec!["e", &target]]);
    assert_eq!(plan.accepts_repost(&repost), Some("alice".to_string()));
}

#[test]
fn accepts_kind_16_generic_repost_with_matching_k_tag() {
    let target = "c".repeat(64);
    let plan = plan_for(&target);
    let repost = event(
        "r2",
        "bob",
        GENERIC_REPOST_KIND,
        vec![vec!["e", &target], vec!["k", "1"]],
    );
    assert_eq!(plan.accepts_repost(&repost), Some("bob".to_string()));
}

#[test]
fn rejects_kind_16_generic_repost_with_non_note_k_tag() {
    let target = "d".repeat(64);
    let plan = plan_for(&target);
    // Discriminated out: the wrapper itself proves it targets a different
    // kind (e.g. 30023), so it must not be admitted as a repost of our
    // kind:1 target even though the wrapper `#e`-tags this id.
    let repost = event(
        "r3",
        "carol",
        GENERIC_REPOST_KIND,
        vec![vec!["e", &target], vec!["k", "30023"]],
    );
    assert_eq!(plan.accepts_repost(&repost), None);
}

#[test]
fn rejects_repost_of_a_different_target() {
    let target = "e".repeat(64);
    let plan = plan_for(&target);
    let other = "f".repeat(64);
    let repost = event("r4", "dave", REPOST_KIND, vec![vec!["e", &other]]);
    assert_eq!(plan.accepts_repost(&repost), None);
}

#[test]
fn rejects_non_repost_kind() {
    let target = "1".repeat(64);
    let plan = plan_for(&target);
    let note = event("n1", "erin", 1, vec![vec!["e", &target]]);
    assert_eq!(plan.accepts_repost(&note), None);
}
