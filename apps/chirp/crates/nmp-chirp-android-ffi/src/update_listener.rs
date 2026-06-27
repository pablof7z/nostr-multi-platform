//! JNI push listener — deleted in M14-1 (#2198).
//!
//! `nativeSetUpdateListener` and `nativeClearUpdateListener` were deleted in
//! M14-0 (#2129). The `UpdatePushListener` type and `UpdateListenerSlot` typedef
//! are now also deleted: the slot was always `None` in production after M14-0 and
//! the only callers were in-crate tests. Update delivery is served exclusively by
//! the UniFFI `AppHandle::set_update_sink` / `clear_update_sink` path in
//! `uniffi_app_loop.rs`.
