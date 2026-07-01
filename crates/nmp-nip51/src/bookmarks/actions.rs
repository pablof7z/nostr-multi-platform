use std::sync::Arc;

use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection,
};
use serde::{Deserialize, Serialize};

use super::{
    action_rejection_message, build_bookmark_list_event, dedupe_snapshot, normalize_item,
    snapshot_contains, BookmarkItem, BookmarkListProjection,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BookmarkUpdateInput {
    pub account_pubkey: String,
    pub item: BookmarkItem,
}

pub struct AddBookmarkAction {
    projection: Arc<BookmarkListProjection>,
}

impl AddBookmarkAction {
    #[must_use]
    pub fn new(projection: Arc<BookmarkListProjection>) -> Self {
        Self { projection }
    }
}

impl ActionModule for AddBookmarkAction {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.nip51.add_bookmark",
            "action.nmp.nip51.add_bookmark",
        );
    type Action = BookmarkUpdateInput;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        let item = normalize_item(&action.item).map_err(ActionRejection::Invalid)?;
        self.projection
            .ensure_active_account(&action.account_pubkey)?;
        if snapshot_contains(
            &self.projection.snapshot_for_account(&action.account_pubkey),
            &item,
        ) {
            return Err(ActionRejection::Conflict(
                "bookmark item is already present in kind:10003".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let item = normalize_item(&action.item)?;
        self.projection
            .ensure_active_account(&action.account_pubkey)
            .map_err(action_rejection_message)?;
        let mut snapshot = self.projection.snapshot_for_account(&action.account_pubkey);
        snapshot.items.push(item);
        let snapshot = dedupe_snapshot(snapshot);
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: build_bookmark_list_event(&snapshot),
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        self.projection
            .replace_snapshot_for_active_account(&action.account_pubkey, snapshot)?;
        Ok(())
    }
}

pub struct RemoveBookmarkAction {
    projection: Arc<BookmarkListProjection>,
}

impl RemoveBookmarkAction {
    #[must_use]
    pub fn new(projection: Arc<BookmarkListProjection>) -> Self {
        Self { projection }
    }
}

impl ActionModule for RemoveBookmarkAction {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.nip51.remove_bookmark",
            "action.nmp.nip51.remove_bookmark",
        );
    type Action = BookmarkUpdateInput;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        let item = normalize_item(&action.item).map_err(ActionRejection::Invalid)?;
        self.projection
            .ensure_active_account(&action.account_pubkey)?;
        if !snapshot_contains(
            &self.projection.snapshot_for_account(&action.account_pubkey),
            &item,
        ) {
            return Err(ActionRejection::Conflict(
                "bookmark item is not present in kind:10003".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let item = normalize_item(&action.item)?;
        self.projection
            .ensure_active_account(&action.account_pubkey)
            .map_err(action_rejection_message)?;
        let mut snapshot = self.projection.snapshot_for_account(&action.account_pubkey);
        snapshot
            .items
            .retain(|candidate| !super::same_item(candidate, &item));
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: build_bookmark_list_event(&snapshot),
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        self.projection
            .replace_snapshot_for_active_account(&action.account_pubkey, snapshot)?;
        Ok(())
    }
}

pub fn register_bookmark_actions(
    app: &mut impl ActionRegistrar,
    projection: Arc<BookmarkListProjection>,
) {
    app.register_default_action(AddBookmarkAction::new(Arc::clone(&projection)));
    app.register_default_action(RemoveBookmarkAction::new(projection));
}
