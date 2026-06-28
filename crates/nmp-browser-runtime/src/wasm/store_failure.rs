//! OPFS-SQLite open-failure taxonomy (#1007 PR-8).
//!
//! The single place that maps a durable-store open failure to a **stable reason
//! string**. These strings are recorded on the kernel's `store_open_failure`
//! diagnostic (via `BrowserAppBuilder::with_store_open_failure` →
//! `KernelReducer::set_store_open_failure`) and surface through the Tier-3
//! snapshot — the exact channel the native LMDB degraded-open path uses.
//!
//! # Why a taxonomy
//!
//! The OPFS SyncAccessHandle-pool VFS (`opfs-sahpool`, ADR-0054) can fail to open
//! for several mutually-distinct reasons that a host UI must distinguish (offer
//! "try a different browser" vs. "close your other tab" vs. "free up space"):
//!
//! | Reason string                          | Cause |
//! |----------------------------------------|-------|
//! | `opfs_store_open_failure: safari_or_sah_pool_unavailable` | Safari < 17.4 or any engine where `createSyncAccessHandle()` / the sahpool VFS is missing. Durable storage is simply unsupported. |
//! | `opfs_store_open_failure: private_browsing` | Private/incognito window where OPFS is blocked by a `SecurityError`. |
//! | `opfs_store_open_failure: quota_denied` | The origin's storage quota was exhausted at open (pool pre-allocation). |
//! | `opfs_store_open_failure: handle_loss` | A `SyncAccessHandle` was lost/invalidated (`InvalidStateError`) — e.g. the OS reclaimed it. |
//! | `opfs_store_open_failure: second_tab_pool_lock` | Another tab already holds durable ownership for this `database_name`, or the exclusive sahpool lock could not be acquired. |
//! | `opfs_store_open_failure: unknown` | An open failure that matched none of the above — never silently dropped. |
//!
//! # Classification source
//!
//! The `nmp-store` wrapper collapses the engine's `SqliteWasmError`
//! (`ModuleInit`/`VfsInstall`/`Open`/`Step`/…) into `StoreError::Io(msg)`, where
//! `msg` is the stringified JS `DOMException` (its `name`/`message`). We classify
//! off that text. The JS exception text is engine-level only (D6: no private
//! event content), so substring-matching it is safe. Matching is
//! case-insensitive and ordered most-specific-first.
//!
//! Always-compiled (not wasm-gated) so the classifier is unit-tested on native.
//! The classifier's only non-test consumer is the wasm-gated
//! `NmpWasmRuntime::prepare_store`, so on native non-test builds the items are
//! exercised only from `#[cfg(test)]` — suppress the resulting dead-code warning
//! exactly as the sibling `core`/`dispatch` modules do.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use nmp_store::StoreError;

/// Stable reason prefix shared by every OPFS open-failure reason, so a host can
/// `split(": ")` on the first separator (mirrors the degraded-mode vocabulary in
/// `docs/wasm-surface.md`). Test-asserted invariant — the production reasons
/// embed it literally.
#[cfg(test)]
const PREFIX: &str = "opfs_store_open_failure";

/// Durable storage unsupported: Safari < 17.4 / no `createSyncAccessHandle` /
/// sahpool VFS missing.
pub(crate) const SAFARI_OR_SAH_POOL_UNAVAILABLE: &str =
    "opfs_store_open_failure: safari_or_sah_pool_unavailable";
/// OPFS blocked by the engine in a private/incognito window.
pub(crate) const PRIVATE_BROWSING: &str = "opfs_store_open_failure: private_browsing";
/// Origin storage quota exhausted during pool pre-allocation.
pub(crate) const QUOTA_DENIED: &str = "opfs_store_open_failure: quota_denied";
/// A SyncAccessHandle was lost / invalidated.
pub(crate) const HANDLE_LOSS: &str = "opfs_store_open_failure: handle_loss";
/// Another tab owns durable mode for this `database_name`.
pub(crate) const SECOND_TAB_POOL_LOCK: &str = "opfs_store_open_failure: second_tab_pool_lock";
/// Open failed for a reason outside the known taxonomy.
pub(crate) const UNKNOWN: &str = "opfs_store_open_failure: unknown";

/// Map a durable-store open failure to a stable reason string (#1007 PR-8).
///
/// Ordered most-specific-first so e.g. a quota error that also mentions "handle"
/// classifies as `quota_denied`. Returns a `&'static str` from the taxonomy
/// above — never the raw exception text, so the diagnostic is deterministic and
/// carries no variable/secret content.
pub(crate) fn classify_open_failure(err: &StoreError) -> &'static str {
    let msg = err.to_string().to_ascii_lowercase();

    // Quota first — a full disk is the most actionable and most specific signal.
    if msg.contains("quota") || msg.contains("quotaexceeded") || msg.contains("no space") {
        return QUOTA_DENIED;
    }
    // Second-tab pool-lock contention: the sahpool is exclusive per origin+name.
    if msg.contains("nomodificationallowed")
        || msg.contains("no modification allowed")
        || msg.contains("already locked")
        || msg.contains("cannot acquire")
        || msg.contains("could not acquire")
        || msg.contains("all sah")
        || msg.contains("pool is locked")
        || msg.contains("another tab")
    {
        return SECOND_TAB_POOL_LOCK;
    }
    // Private browsing / origin-policy denial surfaces as a SecurityError.
    if msg.contains("securityerror")
        || msg.contains("security error")
        || msg.contains("private")
        || msg.contains("not allowed")
    {
        return PRIVATE_BROWSING;
    }
    // Unsupported engine: no createSyncAccessHandle / no sahpool VFS (Safari < 17.4).
    if msg.contains("createsyncaccesshandle")
        || msg.contains("is not a function")
        || msg.contains("undefined is not")
        || msg.contains("not supported")
        || msg.contains("unsupported")
        || msg.contains("not available")
        || msg.contains("notsupportederror")
    {
        return SAFARI_OR_SAH_POOL_UNAVAILABLE;
    }
    // A lost/invalidated SyncAccessHandle.
    if msg.contains("invalidstate")
        || msg.contains("invalid state")
        || msg.contains("access handle")
        || msg.contains("handle")
    {
        return HANDLE_LOSS;
    }
    UNKNOWN
}

#[cfg(test)]
mod tests {
    use super::*;

    fn io(msg: &str) -> StoreError {
        StoreError::Io(msg.to_string())
    }

    #[test]
    fn every_reason_carries_the_shared_prefix() {
        for r in [
            SAFARI_OR_SAH_POOL_UNAVAILABLE,
            PRIVATE_BROWSING,
            QUOTA_DENIED,
            HANDLE_LOSS,
            SECOND_TAB_POOL_LOCK,
            UNKNOWN,
        ] {
            assert!(
                r.starts_with(PREFIX),
                "reason `{r}` must carry the `{PREFIX}` prefix"
            );
            assert!(
                r.contains(": "),
                "reason `{r}` must be host-splittable on `: `"
            );
        }
    }

    #[test]
    fn quota_classifies_first() {
        assert_eq!(
            classify_open_failure(&io("QuotaExceededError: ...")),
            QUOTA_DENIED
        );
        assert_eq!(
            classify_open_failure(&io("the disk has no space left")),
            QUOTA_DENIED
        );
        // Quota wins even if other tokens are present.
        assert_eq!(
            classify_open_failure(&io("QuotaExceededError on access handle")),
            QUOTA_DENIED
        );
    }

    #[test]
    fn second_tab_pool_lock_classifies() {
        assert_eq!(
            classify_open_failure(&io("NoModificationAllowedError: cannot acquire pool")),
            SECOND_TAB_POOL_LOCK
        );
        assert_eq!(
            classify_open_failure(&io("Could not acquire an access handle; another tab open")),
            SECOND_TAB_POOL_LOCK
        );
    }

    #[test]
    fn private_browsing_classifies() {
        assert_eq!(
            classify_open_failure(&io("SecurityError: storage disallowed")),
            PRIVATE_BROWSING
        );
        assert_eq!(
            classify_open_failure(&io("operation is not allowed")),
            PRIVATE_BROWSING
        );
    }

    #[test]
    fn safari_or_sah_unavailable_classifies() {
        assert_eq!(
            classify_open_failure(&io("createSyncAccessHandle is not a function")),
            SAFARI_OR_SAH_POOL_UNAVAILABLE
        );
        assert_eq!(
            classify_open_failure(&io("OPFS is not available in this browser")),
            SAFARI_OR_SAH_POOL_UNAVAILABLE
        );
    }

    #[test]
    fn handle_loss_classifies() {
        assert_eq!(
            classify_open_failure(&io("InvalidStateError: access handle closed")),
            HANDLE_LOSS
        );
    }

    #[test]
    fn unrecognized_open_failure_is_unknown_never_dropped() {
        assert_eq!(classify_open_failure(&io("borked for reasons")), UNKNOWN);
        // A non-Io open failure variant still classifies (defensive default).
        assert_eq!(
            classify_open_failure(&StoreError::CorruptEnv("weird".into())),
            UNKNOWN
        );
    }
}
