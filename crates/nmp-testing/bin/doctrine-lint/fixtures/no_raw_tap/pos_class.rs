//! Positive no_raw_tap CLASS fixture — must trigger a no_raw_tap finding even
//! though it uses NO banned named token.
//!
//! This is a *renamed* reincarnation of the deleted raw event tap: a below-seam
//! `extern "C" fn(*mut c_void, *const c_char)` per-event ingest callback,
//! registered outside the sanctioned `external_event_sink` module. The CLASS
//! check must catch it on the intent token (`raw_tap` / `event_tap`) + the
//! C-ABI per-event callback signature, independent of the symbol name.

use std::ffi::{c_char, c_void};

/// A renamed per-event tap — different name, same forbidden shape. The
/// `raw_tap` intent token plus the C per-event callback signature trip the
/// CLASS rule.
pub type SneakyRawTapCallback = extern "C" fn(ctx: *mut c_void, raw_tap: *const c_char);

pub fn register_sneaky(_cb: SneakyRawTapCallback) {}
