use super::*;

const TARGET: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const OTHER: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn target() -> ZapTarget {
    ZapTarget::event(TARGET).expect("valid hex64 id")
}

fn receipt(id: &str, target_id: &str, sender: Option<&str>, amount_tag: Option<u64>) -> KernelEvent {
    let description = amount_tag.map(|amount| {
        let sender_json = sender
            .map(|s| format!("\"pubkey\":\"{s}\",\"tags\":[[\"amount\",\"{amount}\"]]"))
            .unwrap_or_else(|| format!("\"tags\":[[\"amount\",\"{amount}\"]]"));
        format!("{{{sender_json}}}")
    });
    let mut tags = vec![
        vec!["p".to_string(), "recipient".to_string()],
        vec!["e".to_string(), target_id.to_string()],
    ];
    if let Some(desc) = description {
        tags.push(vec!["description".to_string(), desc]);
    }
    KernelEvent {
        id: id.to_string(),
        author: "ln_provider".to_string(),
        kind: KIND_ZAP_RECEIPT,
        created_at: 1,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn filter_json_selects_kind_9735_and_the_target_e_tag() {
    let plan = ZapReadPlan::new(target());
    let json = plan.filter_json();
    assert!(json.contains("\"kinds\":[9735]"));
    assert!(json.contains(&format!("\"#e\":[\"{TARGET}\"]")));
}

#[test]
fn accepts_a_receipt_zapping_the_target() {
    let plan = ZapReadPlan::new(target());
    let event = receipt("Z1", TARGET, Some("alice"), Some(15_000));
    let record = plan.accepts(&event).expect("receipt zaps the target");
    assert_eq!(record.zapped_event_id.as_deref(), Some(TARGET));
    assert_eq!(record.sender_pubkey.as_deref(), Some("alice"));
    assert_eq!(record.amount_msats, Some(15_000));
}

#[test]
fn rejects_a_receipt_zapping_a_different_target() {
    let plan = ZapReadPlan::new(target());
    let event = receipt("Z1", OTHER, Some("alice"), Some(15_000));
    assert!(plan.accepts(&event).is_none());
}

#[test]
fn rejects_a_non_receipt_kind() {
    let plan = ZapReadPlan::new(target());
    let mut event = receipt("Z1", TARGET, Some("alice"), Some(15_000));
    event.kind = 1;
    assert!(plan.accepts(&event).is_none());
}

#[test]
fn accepts_an_anonymous_receipt_with_no_discoverable_sender() {
    let plan = ZapReadPlan::new(target());
    let event = receipt("Z1", TARGET, None, None);
    let record = plan.accepts(&event).expect("anonymous receipt is still a valid zap");
    assert_eq!(record.sender_pubkey, None);
}
