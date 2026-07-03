use trellis_core::ResourceCommand;

use crate::{
    XrayInterestDescriptor, XrayProjectionContext, XrayReceipt, XrayReceiptEventKind,
    XrayTransactionMarker,
};

/// Adapter trait for Trellis command payloads that can describe NMP interests.
///
/// The returned descriptor is deliberately NMP-owned. It carries stable labels
/// and summaries for diagnostics without exposing Trellis graph primitives to
/// downstream tools.
pub trait TrellisReceiptPayload {
    fn interest_descriptor(&self) -> Option<XrayInterestDescriptor>;
}

/// Convert ordered Trellis resource commands into NMP-owned X-Ray receipts.
#[must_use]
pub fn receipts_from_trellis_commands<C>(
    context: &XrayProjectionContext,
    transaction: XrayTransactionMarker,
    commands: &[ResourceCommand<C>],
) -> Vec<XrayReceipt>
where
    C: TrellisReceiptPayload,
{
    commands
        .iter()
        .map(|command| {
            let (event, resource_key, interest) = match command {
                ResourceCommand::Open { key, command, .. } => (
                    XrayReceiptEventKind::Open,
                    key.as_str().to_string(),
                    command.interest_descriptor(),
                ),
                ResourceCommand::Replace { key, command, .. } => (
                    XrayReceiptEventKind::Replace,
                    key.as_str().to_string(),
                    command.interest_descriptor(),
                ),
                ResourceCommand::Refresh { key, command, .. } => (
                    XrayReceiptEventKind::Refresh,
                    key.as_str().to_string(),
                    command.interest_descriptor(),
                ),
                ResourceCommand::Close { key, .. } => {
                    (XrayReceiptEventKind::Close, key.as_str().to_string(), None)
                }
            };
            XrayReceipt::new(context.clone(), transaction, event, resource_key, interest)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use trellis_core::{Graph, ResourceKey, ResourcePlan};

    use super::*;

    #[derive(Clone)]
    struct DemoCommand(&'static str);

    impl TrellisReceiptPayload for DemoCommand {
        fn interest_descriptor(&self) -> Option<XrayInterestDescriptor> {
            Some(XrayInterestDescriptor::new(
                format!("interest:{}", self.0),
                "global",
                format!("authors={}", self.0),
                "active-follow-timeline",
            ))
        }
    }

    fn test_scope() -> trellis_core::ScopeId {
        let mut graph = Graph::<DemoCommand>::new_with_command_type();
        let mut tx = graph.begin_transaction().unwrap();
        tx.create_scope("xray-test").unwrap()
    }

    #[test]
    fn trellis_commands_convert_to_ordered_nmp_owned_receipts() {
        let scope = test_scope();
        let mut plan = ResourcePlan::new();
        plan.open(
            ResourceKey::new("alpha".to_string()),
            scope,
            DemoCommand("alice"),
        );
        plan.replace(
            ResourceKey::new("bravo".to_string()),
            scope,
            DemoCommand("bob"),
        );
        plan.refresh(
            ResourceKey::new("charlie".to_string()),
            scope,
            DemoCommand("carol"),
        );
        plan.close(ResourceKey::new("delta".to_string()), scope);

        let context =
            XrayProjectionContext::new("app.feed.home", "home-feed", "owner:feed", "sync");
        let receipts = receipts_from_trellis_commands(
            &context,
            XrayTransactionMarker::new(11, 4),
            plan.commands(),
        );

        assert_eq!(receipts.len(), 4);
        assert_eq!(receipts[0].event, XrayReceiptEventKind::Open);
        assert_eq!(receipts[1].event, XrayReceiptEventKind::Replace);
        assert_eq!(receipts[2].event, XrayReceiptEventKind::Refresh);
        assert_eq!(receipts[3].event, XrayReceiptEventKind::Close);
        assert_eq!(receipts[0].resource_key, "alpha");
        assert_eq!(
            receipts[0].interest.as_ref().unwrap().provenance,
            "active-follow-timeline"
        );
        assert!(receipts[3].interest.is_none());
    }

    #[test]
    fn serialized_receipts_do_not_expose_raw_trellis_vocabulary() {
        let scope = test_scope();
        let mut plan = ResourcePlan::new();
        plan.open(
            ResourceKey::new("alpha".to_string()),
            scope,
            DemoCommand("alice"),
        );

        let context =
            XrayProjectionContext::new("app.feed.home", "home-feed", "owner:feed", "sync");
        let receipts = receipts_from_trellis_commands(
            &context,
            XrayTransactionMarker::new(11, 4),
            plan.commands(),
        );
        let value: Value = serde_json::to_value(&receipts).unwrap();
        let json = serde_json::to_string(&value).unwrap();

        assert!(json.contains("app.feed.home"));
        assert!(!json.contains("trellis_core"));
        assert!(!json.contains("ResourceKey"));
        assert!(!json.contains("ResourcePlan"));
    }
}
