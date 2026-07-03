//! `nmp-wallet` typed action builders (#2920, epic #2864): `select_backend`
//! plus the Cashu and nutzap families. Kept separate so `table.rs` remains an
//! aggregate list under the hand-authored file-size gate.

use super::{ActionBuilder, FieldKind, PayloadField};

pub(super) const WALLET_SELECT_BACKEND: ActionBuilder = ActionBuilder {
    namespace: "nmp.wallet.select_backend",
    method: "walletSelectBackend",
    fields: &[PayloadField {
        name: "backendId",
        kind: FieldKind::Str,
        optional: false,
    }],
    doc: "Select the preferred registered wallet backend by id.",
};

pub(super) const WALLET_CASHU_CREATE: ActionBuilder = ActionBuilder {
    namespace: "nmp.wallet.cashu.create",
    method: "walletCashuCreate",
    fields: &[PayloadField {
        name: "mint",
        kind: FieldKind::Str,
        optional: false,
    }],
    doc: "Create a Cashu wallet against the given mint.",
};

pub(super) const WALLET_CASHU_RECOVER: ActionBuilder = ActionBuilder {
    namespace: "nmp.wallet.cashu.recover",
    method: "walletCashuRecover",
    fields: &[],
    doc: "Recover a Cashu wallet (no backend implements this yet; always rejects).",
};

pub(super) const WALLET_CASHU_DEPOSIT_QUOTE: ActionBuilder = ActionBuilder {
    namespace: "nmp.wallet.cashu.deposit_quote",
    method: "walletCashuDepositQuote",
    fields: &[
        PayloadField {
            name: "mint",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "amountSats",
            kind: FieldKind::Ulong,
            optional: false,
        },
    ],
    doc: "Request a Cashu deposit quote from a mint for an amount in satoshis.",
};

pub(super) const WALLET_CASHU_COMPLETE_DEPOSIT: ActionBuilder = ActionBuilder {
    namespace: "nmp.wallet.cashu.complete_deposit",
    method: "walletCashuCompleteDeposit",
    fields: &[PayloadField {
        name: "quoteId",
        kind: FieldKind::Str,
        optional: false,
    }],
    doc: "Complete a previously requested Cashu deposit by quote id.",
};

pub(super) const WALLET_NUTZAP_PUBLISH_INFO: ActionBuilder = ActionBuilder {
    namespace: "nmp.wallet.nutzap.publish_info",
    method: "walletNutzapPublishInfo",
    fields: &[],
    doc: "Publish this account's kind:10019 nutzap info event.",
};

pub(super) const WALLET_NUTZAP_SEND: ActionBuilder = ActionBuilder {
    namespace: "nmp.wallet.nutzap.send",
    method: "walletNutzapSend",
    fields: &[
        PayloadField {
            name: "recipientPubkey",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "amountSats",
            kind: FieldKind::Ulong,
            optional: false,
        },
        // Optional target event id (a nutzap on a note); absent -> a
        // top-level nutzap to the recipient's pubkey.
        PayloadField {
            name: "targetEventId",
            kind: FieldKind::Str,
            optional: true,
        },
    ],
    doc: "Send a nutzap to a recipient, optionally targeting a specific event.",
};

pub(super) const WALLET_NUTZAP_REDEEM: ActionBuilder = ActionBuilder {
    namespace: "nmp.wallet.nutzap.redeem",
    method: "walletNutzapRedeem",
    fields: &[PayloadField {
        name: "eventId",
        kind: FieldKind::Str,
        optional: false,
    }],
    doc: "Redeem a kind:9321 nutzap event's proofs into this wallet.",
};
