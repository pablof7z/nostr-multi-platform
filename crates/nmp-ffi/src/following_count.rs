//! Live "following count" read for host profile screens.
//!
//! The host profile header shows how many accounts the active user follows.
//! That number is the count of distinct `p` tags in the active account's
//! current kind:3 contact list. This C-ABI door reads it synchronously from
//! the kernel's published event store — the SAME store every locally-published
//! event is ingested into before its observer fan-out (read-your-writes,
//! ADR-0057) — so a kind:3 just written by `nmp.follow` / `nmp.follow_many`
//! (e.g. onboarding applying a follow pack) is reflected immediately, with no
//! relay round-trip required. This is what lets a brand-new account that just
//! selected a follow pack show a non-zero "Following" count even on a device
//! with no relay connectivity: the count is a LOCAL store read, not a network
//! observation.

use super::{app_ref, NmpApp};

/// Return the number of accounts the active user currently follows.
///
/// Reads the active account's latest kind:3 from the kernel's published event
/// store and counts its distinct, hex-valid `p` tags.
///
/// Returns:
/// - `>= 0` — the active account HAS a kind:3 in the store; the value is the
///   number of distinct follows (`0` for an explicit empty contact list).
/// - `-1` — there is no active account, the store has not been published yet
///   (pre-`nmp_app_start`), a lock is poisoned, or the active account has not
///   published a kind:3 yet. Hosts render `-1` as `0` for display; it is kept
///   distinct from `0` so a caller could tell "no list yet" from "loaded but
///   empty" if it wanted to.
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_active_following_count(app: *mut NmpApp) -> i64 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> i64 {
        let Some(app) = app_ref(app) else {
            return -1;
        };
        // Read the active account's hex pubkey from the kernel-published slot.
        let Some(author_hex) = app
            .active_account_handle()
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
        else {
            return -1;
        };
        match nmp_core::slots::following_count_from_store(&app.event_store_handle(), &author_hex) {
            Some(n) => i64::try_from(n).unwrap_or(i64::MAX),
            None => -1,
        }
    }));
    result.unwrap_or(-1)
}
