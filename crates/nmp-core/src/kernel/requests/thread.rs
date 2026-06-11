//! Thread view request builders.
//!
//! V-68 / V-112 (ADR-0042): `open_thread`, `close_thread`,
//! `prepare_thread_requests`, `enqueue_thread_id`,
//! `enqueue_thread_reply_target`, `maybe_open_thread_hydration` all deleted.
//!
//! Thread view state now lives in the per-app `FlatFeed` registered by
//! `nmp_app_chirp_open_thread_feed` (apps/chirp/nmp-app-chirp/src/ffi/interest_feed.rs).
//! Kernel admission is via `open_interest` / `close_interest` (ADR-0042).

#[cfg(test)]
mod tests {
    #[test]
    fn thread_rs_placeholder() {
        // File retained as a stub; the actual thread builders were deleted
        // as part of V-68 / V-112 (ADR-0042). This test prevents rustc from
        // complaining about an empty #[cfg(test)] mod block.
    }
}
