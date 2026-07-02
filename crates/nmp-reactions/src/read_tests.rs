use super::*;

const ROOT: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[test]
fn filter_carries_reaction_kind_and_e_only() {
    let target = ReactionTarget::event(ROOT).unwrap();
    let filter = reaction_filter_json(&target);
    let v: serde_json::Value = serde_json::from_str(&filter).unwrap();
    assert_eq!(v["kinds"], serde_json::json!([7]));
    assert_eq!(v["#e"], serde_json::json!([ROOT]));
    assert!(v.get("relay_pin").is_none());
    // The interest planner must accept the composed filter — same proof the
    // group-scoped reaction read's filter carries (group_feed::reactions).
    assert!(nmp_planner::InterestShape::from_filter_json(&filter).is_some());
}
