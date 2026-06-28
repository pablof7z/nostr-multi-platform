// Lock-ordering tests for the JNI push-listener path were deleted in #2198.
// `UpdatePushListener` and `push_listener` slot were removed (M14-1): the slot
// was always None in production after M14-0 (#2129) and the only callers were
// these tests.  Update delivery is served exclusively by the UniFFI
// `AppHandle::set_update_sink` path (`uniffi_app_loop.rs`).
