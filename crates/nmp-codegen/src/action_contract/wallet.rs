//! `nmp-wallet` action contract entries — `select_backend` plus the Cashu and
//! nutzap families (#2920, epic #2864).
//!
//! Extracted from `table.rs` to keep each file under the 500-LOC hard cap.
//! Each constant is a named `ActionContract` entry assembled into
//! `ACTION_CONTRACT` in `table.rs` by name.
//!
//! `nmp.wallet.{connect,disconnect,pay_invoice}` stay in `table.rs` itself —
//! those three are `nmp-nip47`'s own long-standing entries (see that crate's
//! module docs for why `nmp-wallet` does not re-register them).

use super::{
    ActionContract, ActionDefaultTier, BuilderSupport, PublicReExportPolicy, TypedDispatchPolicy,
};

const PUBLIC_REEXPORT: PublicReExportPolicy = PublicReExportPolicy::OwnerCratePayload;
const TYPED_ONLY: TypedDispatchPolicy = TypedDispatchPolicy::TypedOnly;

pub const WALLET_SELECT_BACKEND: ActionContract = ActionContract {
    namespace: "nmp.wallet.select_backend",
    producer: "nmp-wallet action",
    module_type: "nmp_wallet::action::SelectBackendModule",
    payload_type: "nmp_wallet::SelectBackendAction",
    owner_claim: "action.nmp.wallet.select_backend",
    schema_id: "nmp.wallet.select_backend",
    schema_path: "crates/nmp-wallet/schema/select_backend.fbs",
    root_type: "SelectBackendPayload",
    schema_version: 1,
    file_identifier: "NWSB",
    default_tier: ActionDefaultTier::Wallet,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const WALLET_CASHU_CREATE: ActionContract = ActionContract {
    namespace: "nmp.wallet.cashu.create",
    producer: "nmp-wallet action",
    module_type: "nmp_wallet::action::CashuCreateModule",
    payload_type: "nmp_wallet::CashuCreateAction",
    owner_claim: "action.nmp.wallet.cashu",
    schema_id: "nmp.wallet.cashu.create",
    schema_path: "crates/nmp-wallet/schema/cashu_create.fbs",
    root_type: "CashuCreatePayload",
    schema_version: 1,
    file_identifier: "NWCC",
    default_tier: ActionDefaultTier::Wallet,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const WALLET_CASHU_RECOVER: ActionContract = ActionContract {
    namespace: "nmp.wallet.cashu.recover",
    producer: "nmp-wallet action",
    module_type: "nmp_wallet::action::CashuRecoverModule",
    payload_type: "nmp_wallet::CashuRecoverAction",
    owner_claim: "action.nmp.wallet.cashu",
    schema_id: "nmp.wallet.cashu.recover",
    schema_path: "crates/nmp-wallet/schema/cashu_recover.fbs",
    root_type: "CashuRecoverPayload",
    schema_version: 1,
    file_identifier: "NWCR",
    default_tier: ActionDefaultTier::Wallet,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const WALLET_CASHU_DEPOSIT_QUOTE: ActionContract = ActionContract {
    namespace: "nmp.wallet.cashu.deposit_quote",
    producer: "nmp-wallet action",
    module_type: "nmp_wallet::action::CashuDepositQuoteModule",
    payload_type: "nmp_wallet::CashuDepositQuoteAction",
    owner_claim: "action.nmp.wallet.cashu",
    schema_id: "nmp.wallet.cashu.deposit_quote",
    schema_path: "crates/nmp-wallet/schema/cashu_deposit_quote.fbs",
    root_type: "CashuDepositQuotePayload",
    schema_version: 1,
    file_identifier: "NWDQ",
    default_tier: ActionDefaultTier::Wallet,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const WALLET_CASHU_COMPLETE_DEPOSIT: ActionContract = ActionContract {
    namespace: "nmp.wallet.cashu.complete_deposit",
    producer: "nmp-wallet action",
    module_type: "nmp_wallet::action::CashuCompleteDepositModule",
    payload_type: "nmp_wallet::CashuCompleteDepositAction",
    owner_claim: "action.nmp.wallet.cashu",
    schema_id: "nmp.wallet.cashu.complete_deposit",
    schema_path: "crates/nmp-wallet/schema/cashu_complete_deposit.fbs",
    root_type: "CashuCompleteDepositPayload",
    schema_version: 1,
    file_identifier: "NWCD",
    default_tier: ActionDefaultTier::Wallet,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const WALLET_NUTZAP_PUBLISH_INFO: ActionContract = ActionContract {
    namespace: "nmp.wallet.nutzap.publish_info",
    producer: "nmp-wallet action",
    module_type: "nmp_wallet::action::NutzapPublishInfoModule",
    payload_type: "nmp_wallet::NutzapPublishInfoAction",
    owner_claim: "action.nmp.wallet.nutzap",
    schema_id: "nmp.wallet.nutzap.publish_info",
    schema_path: "crates/nmp-wallet/schema/nutzap_publish_info.fbs",
    root_type: "NutzapPublishInfoPayload",
    schema_version: 1,
    file_identifier: "NWPI",
    default_tier: ActionDefaultTier::Wallet,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const WALLET_NUTZAP_SEND: ActionContract = ActionContract {
    namespace: "nmp.wallet.nutzap.send",
    producer: "nmp-wallet action",
    module_type: "nmp_wallet::action::NutzapSendModule",
    payload_type: "nmp_wallet::NutzapSendAction",
    owner_claim: "action.nmp.wallet.nutzap",
    schema_id: "nmp.wallet.nutzap.send",
    schema_path: "crates/nmp-wallet/schema/nutzap_send.fbs",
    root_type: "NutzapSendPayload",
    schema_version: 1,
    file_identifier: "NWNS",
    default_tier: ActionDefaultTier::Wallet,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};

pub const WALLET_NUTZAP_REDEEM: ActionContract = ActionContract {
    namespace: "nmp.wallet.nutzap.redeem",
    producer: "nmp-wallet action",
    module_type: "nmp_wallet::action::NutzapRedeemModule",
    payload_type: "nmp_wallet::NutzapRedeemAction",
    owner_claim: "action.nmp.wallet.nutzap",
    schema_id: "nmp.wallet.nutzap.redeem",
    schema_path: "crates/nmp-wallet/schema/nutzap_redeem.fbs",
    root_type: "NutzapRedeemPayload",
    schema_version: 1,
    file_identifier: "NWNR",
    default_tier: ActionDefaultTier::Wallet,
    builder_support: BuilderSupport::GeneratedFlatTable,
    public_re_export: PUBLIC_REEXPORT,
    typed_dispatch: TYPED_ONLY,
};
