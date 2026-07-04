//! `nmp-wallet` product-surface user-facing error codes (issue #1682
//! pattern), namespaced `wallet_*` — distinct from
//! `backend::cashu::ui_codes`'s `wallet_cashu_*` codes, which are that
//! backend's own internal failure vocabulary. These codes are raised by the
//! backend-selection/dispatch layer itself, before any backend is reached.

/// No registered backend advertises the capability the dispatched
/// `WalletIntent` requires. Absent capability is a user-visible, structured
/// failure — never a silent no-op and never a panic.
pub const NO_CAPABLE_BACKEND: &str = "wallet_no_capable_backend";

/// `nmp.wallet.select_backend` named a `backend_id` no registered backend
/// carries.
pub const UNKNOWN_BACKEND: &str = "wallet_unknown_backend";

/// More than one registered backend advertises the required capability and no
/// preferred backend has been selected to break the tie. Unreachable with
/// today's two backends (their capability sets are disjoint — see
/// `selector::tests`), kept as a fail-closed guard for when a future backend
/// overlaps (e.g. Cashu melt implementing `pay_bolt11`).
pub const AMBIGUOUS_BACKEND_SELECTION: &str = "wallet_ambiguous_backend_selection";

