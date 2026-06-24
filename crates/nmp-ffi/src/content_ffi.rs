//! Pure content-tokenizer C-ABI.
//!
//! This is the FFI wrapper around `nmp-content`'s single tokenizer and wire
//! projection. It does not resolve entities or mutate kernel state. Hosts that
//! want live profile/event data use the existing `nmp_app_resolve_ref` /
//! `nmp_app_release_ref` seam for the `WireNostrUri.primary_id` values emitted
//! here.

use crate::c_string_argument;
use nmp_content::{tokenize, tokenize_with_kind, RenderMode};
use serde::Serialize;
use std::ffi::{c_char, c_int, CString};

#[derive(Serialize)]
struct TokenizeSuccess {
    ok: bool,
    tree: nmp_content::ContentTreeWire,
}

#[derive(Serialize)]
struct TokenizeError {
    ok: bool,
    error: &'static str,
}

const MODE_PLAIN: c_int = 0;
const MODE_MARKDOWN: c_int = 1;
const MODE_AUTO: c_int = 2;

/// Tokenize Nostr content and return the FFI-stable `ContentTreeWire` JSON.
///
/// `mode` values:
/// * `0` - plain text inline tokenization.
/// * `1` - Markdown block + inline tokenization.
/// * `2` - auto mode; `kind` selects Markdown for NIP-23/NIP-54 content,
///   otherwise plain text.
///
/// `tags_json` is optional. When supplied, it must be a JSON `[[string]]`
/// event-tag array and is used for NIP-30 emoji resolution.
///
/// D6: never returns NULL. Invalid input returns `{"ok":false,"error":"..."}`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_content_tokenize_text(
    content: *const c_char,
    tags_json: *const c_char,
    mode: c_int,
    kind: u32,
) -> *mut c_char {
    let output = match tokenize_text_json(content, tags_json, mode, kind) {
        Ok(json) => json,
        Err(error) => error_json(error),
    };
    into_c_string(output)
}

fn tokenize_text_json(
    content: *const c_char,
    tags_json: *const c_char,
    mode: c_int,
    kind: u32,
) -> Result<String, &'static str> {
    let content = c_string_argument(content).ok_or("invalid-input")?;
    let tags = decode_tags(tags_json)?;
    let mode = decode_mode(mode).ok_or("invalid-mode")?;
    let tree = if mode == RenderMode::Auto {
        tokenize_with_kind(&content, &tags, mode, kind)
    } else {
        tokenize(&content, &tags, mode)
    }
    .to_wire();
    serde_json::to_string(&TokenizeSuccess { ok: true, tree }).map_err(|_| "serialization-failed")
}

fn decode_tags(ptr: *const c_char) -> Result<Vec<Vec<String>>, &'static str> {
    let Some(raw) = c_string_argument(ptr) else {
        return Ok(Vec::new());
    };
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw).map_err(|_| "invalid-tags")
}

fn decode_mode(mode: c_int) -> Option<RenderMode> {
    match mode {
        MODE_PLAIN => Some(RenderMode::Plain),
        MODE_MARKDOWN => Some(RenderMode::Markdown),
        MODE_AUTO => Some(RenderMode::Auto),
        _ => None,
    }
}

fn error_json(error: &'static str) -> String {
    serde_json::to_string(&TokenizeError { ok: false, error })
        .unwrap_or_else(|_| r#"{"ok":false,"error":"serialization-failed"}"#.to_string())
}

fn into_c_string(value: String) -> *mut c_char {
    CString::new(value)
        .unwrap_or_else(|_| c"{\"ok\":false,\"error\":\"serialization-failed\"}".to_owned())
        .into_raw()
}

#[cfg(test)]
#[path = "content_ffi_tests.rs"]
mod tests;
