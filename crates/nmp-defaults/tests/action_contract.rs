//! #1939 — integration gates for the neutral typed action contract.
//!
//! These checks tie the codegen contract to the real default composition: each
//! row must match the module namespace, the public `nmp-defaults::action_payloads`
//! re-export, and the live action registry after `register_defaults`.

use std::collections::BTreeSet;

use nmp_core::substrate::{ActionModule, ActionPayload};
mod common;
use common::*;

fn assert_contract<M, A>(namespace: &'static str) -> &'static str
where
    M: ActionModule,
    A: ActionPayload,
{
    let contract = nmp_codegen::action_contract_for(namespace);
    assert_eq!(contract.namespace, M::NAMESPACE);
    assert_eq!(contract.schema_id, A::SCHEMA_ID);
    assert_eq!(contract.schema_version, A::SCHEMA_VERSION);
    namespace
}

#[test]
fn contract_matches_modules_and_default_payload_reexports() {
    use nmp_defaults::action_payloads;

    let checked = [
        assert_contract::<nmp_core::publish::PublishModule, action_payloads::PublishAction>(
            "nmp.publish",
        ),
        assert_contract::<nmp_router::PublishRelayListAction, action_payloads::PublishRelayListInput>(
            "nmp.nip65.publish_relay_list",
        ),
        assert_contract::<nmp_router::BlockRelayAction, action_payloads::BlockRelayInput>(
            "nmp.nip51.block_relay",
        ),
        assert_contract::<nmp_router::UnblockRelayAction, action_payloads::UnblockRelayInput>(
            "nmp.nip51.unblock_relay",
        ),
        assert_contract::<nmp_nip02::FollowModule, action_payloads::PubkeyAction>("nmp.follow"),
        assert_contract::<nmp_nip02::UnfollowModule, action_payloads::PubkeyAction>("nmp.unfollow"),
        assert_contract::<nmp_nip02::FollowManyModule, action_payloads::FollowManyAction>(
            "nmp.follow_many",
        ),
        assert_contract::<nmp_nip25::ReactModule, action_payloads::ReactAction>("nmp.nip25.react"),
        assert_contract::<nmp_nip25::UnreactModule, action_payloads::UnreactAction>(
            "nmp.nip25.unreact",
        ),
        assert_contract::<nmp_nip18::RepostModule, action_payloads::RepostAction>(
            "nmp.nip18.repost",
        ),
        assert_contract::<nmp_nip18::QuoteRepostModule, action_payloads::QuoteRepostAction>(
            "nmp.nip18.quote_repost",
        ),
        assert_contract::<nmp_nip51::AddBookmarkAction, action_payloads::BookmarkUpdateInput>(
            "nmp.nip51.add_bookmark",
        ),
        assert_contract::<nmp_nip51::RemoveBookmarkAction, action_payloads::BookmarkUpdateInput>(
            "nmp.nip51.remove_bookmark",
        ),
        assert_contract::<
            nmp_nip51::AddBookmarkSetItemAction,
            action_payloads::BookmarkSetUpdateInput,
        >("nmp.nip51.add_bookmark_set_item"),
        assert_contract::<
            nmp_nip51::RemoveBookmarkSetItemAction,
            action_payloads::BookmarkSetUpdateInput,
        >("nmp.nip51.remove_bookmark_set_item"),
        assert_contract::<
            nmp_nip51::PublishWebBookmarkAction,
            action_payloads::PublishWebBookmarkInput,
        >("nmp.nip51.publish_web_bookmark"),
        assert_contract::<nmp_replies::ReplyModule, action_payloads::ReplyAction>(
            "nmp.replies.reply",
        ),
        assert_contract::<nmp_nip17::SendDmAction, action_payloads::SendDmInput>("nmp.nip17.send"),
        assert_contract::<
            nmp_nip17::HydratePeerRelayListAction,
            action_payloads::HydratePeerRelayListInput,
        >("nmp.nip17.hydrate_peer_relay_list"),
        assert_contract::<
            nmp_nip17::PublishDmRelayListAction,
            action_payloads::PublishDmRelayListInput,
        >("nmp.nip17.publish_relay_list"),
        assert_contract::<nmp_nip84::PublishHighlightModule, action_payloads::PublishHighlightAction>(
            "nmp.nip84.publish_highlight",
        ),
        // Wallet (opt-in via `with_wallet`; nmp_nip47 available via the
        // `native` default feature). Not in `action_payloads` (only default
        // registrations appear there), so checked directly from nmp_nip47.
        assert_contract::<nmp_nip47::WalletConnectModule, nmp_nip47::WalletConnectAction>(
            "nmp.wallet.connect",
        ),
        assert_contract::<nmp_nip47::WalletDisconnectModule, nmp_nip47::WalletDisconnectAction>(
            "nmp.wallet.disconnect",
        ),
        assert_contract::<nmp_nip47::WalletPayInvoiceModule, nmp_nip47::WalletAction>(
            "nmp.wallet.pay_invoice",
        ),
    ];

    let checked: BTreeSet<&str> = checked.into_iter().collect();
    // `ActionDefaultTier::Marmot` is a feature-gated dep (`nmp-marmot`) not
    // available in `nmp-defaults`. `ActionDefaultTier::Zaps` is post-v1/private
    // (#2318). `ActionDefaultTier::ComponentRegistered` covers crates
    // (nmp-blossom, nmp-relations) wired at app-assembly time, not by
    // nmp-defaults. Filter these from the contract set so the set-equality
    // assertion does not require those crates as deps here.
    let contract: BTreeSet<&str> = nmp_codegen::ACTION_CONTRACT
        .iter()
        .filter(|c| {
            c.default_tier != nmp_codegen::ActionDefaultTier::Marmot
                && c.default_tier != nmp_codegen::ActionDefaultTier::Zaps
                && c.default_tier != nmp_codegen::ActionDefaultTier::ComponentRegistered
        })
        .map(|c| c.namespace)
        .collect();
    assert_eq!(
        checked, contract,
        "every action contract row must be checked against its module \
         namespace and payload type (wallet rows checked via nmp_nip47 directly; \
         zap rows excluded — post-v1/private per #2318; marmot rows excluded — \
         nmp-marmot is a feature-gated dep not available in nmp-defaults; \
         component-registered rows excluded — wired at app-assembly time, not \
         by nmp-defaults)"
    );
}

#[test]
fn component_registered_contract_rows_match_available_modules() {
    assert_contract::<nmp_core::browse::BrowseRelayModule, nmp_core::browse::BrowseRelayAction>(
        "nmp.browse_relay",
    );
    assert_contract::<
        nmp_defaults::topic_articles::TopicArticlesModule,
        nmp_defaults::topic_articles::TopicArticlesAction,
    >("nmp.app.topic_articles");
}

#[test]
fn live_default_action_registry_matches_contract() {
    let app = new_app_ptr();
    assert!(!app.is_null(), "nmp_app_new returned null");
    let app_mut = unsafe { &mut *app };

    nmp_defaults::register_defaults(app_mut);

    let registered = app_mut.registered_action_namespaces();
    let expected: Vec<String> = nmp_codegen::canonical_default_action_namespaces()
        .into_iter()
        .map(str::to_string)
        .collect();
    assert_eq!(
        registered, expected,
        "register_defaults must register exactly the default action contract"
    );

    free_app_ptr(app);
}
