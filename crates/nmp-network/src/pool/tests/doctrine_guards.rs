//! Source-level doctrine guards for the `pool` module:
//! 1. `nmp-network` must never name the planner-side `AuthGate`, the
//!    kind:22242 AUTH event, or the per-relay `RelayAuthState` enum —
//!    those belong to `nmp-core::subs::AuthGate` and `nmp-nip42`
//!    respectively.
//! 2. The expensive `Message → RelayFrame` conversion (which JSON-parses
//!    every text frame for NIP-42 AUTH pre-classification) must run
//!    OUTSIDE the `PoolInner` lock, so it can never block concurrent
//!    `Pool::send` calls.

// `classify_text_frame` behaviour is tested next to its implementation in
// `pool::frame`; the crate-wide doctrine guard below stays here.

/// Doctrine guard: `nmp-network` MUST NOT name the planner-side
/// `AuthGate`, the kind:22242 event, or the per-relay
/// `RelayAuthState` enum anywhere — those belong to
/// `nmp-core::subs::AuthGate` and `nmp-nip42` respectively. This test
/// greps the crate's own source tree at test time so future drift
/// (someone reaching for `nmp-core::subs::AuthGate` from inside the
/// transport layer) trips a hard failure rather than silently
/// re-entangling the layers.
///
/// Bare references in comments are allowed (and exist today to point
/// readers at the canonical home); the guard only rejects code
/// references.
#[test]
fn auth_gate_and_22242_are_not_named_in_this_crate() {
    use std::path::Path;
    let crate_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let this_file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("pool")
        .join("tests")
        .join("doctrine_guards.rs");
    let mut offenders: Vec<String> = Vec::new();
    walk_rs_files(&crate_src, &mut |path, contents| {
        // The test file itself has to name the forbidden tokens to test
        // for their absence; skip it.
        if path == this_file {
            return;
        }
        for (lineno, line) in contents.lines().enumerate() {
            // Strip trivial trailing comments so a `// AuthGate lives in
            // nmp-core::subs` doc-comment doesn't trip the guard.
            let code = line.split("//").next().unwrap_or("");
            // Forbidden semantic tokens:
            //   - `AuthGate` (the pause/replay FSM)
            //   - `22242`    (the kind:22242 AUTH event id)
            //   - `RelayAuthState` (the per-relay FSM enum)
            //   - `build_auth_event` (the kind:22242 builder)
            for needle in ["AuthGate", "22242", "RelayAuthState", "build_auth_event"] {
                if code.contains(needle) {
                    offenders.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        lineno + 1,
                        line.trim()
                    ));
                }
            }
        }
    });
    assert!(
        offenders.is_empty(),
        "nmp-network must not name AuthGate / kind:22242 / RelayAuthState in code \
         (the FSM lives in `nmp-core::subs::AuthGate` and the event builder lives \
         in `nmp-nip42::build_auth_event`); offenders:\n{}",
        offenders.join("\n")
    );
}

/// Defect 1 guard — the expensive `tungstenite::Message → RelayFrame`
/// conversion (which JSON-parses every text frame for NIP-42 AUTH
/// pre-classification) must run OUTSIDE the `PoolInner` lock, so it can never
/// block concurrent `Pool::send` calls (which take the same lock).
///
/// Structurally: `tungstenite_to_relay_frame` must be called only inside
/// `prepare_event` (the off-lock pre-translation step) and NEVER inside
/// `apply_prepared` (the O(1) under-lock step). A regression that moves the
/// conversion back under the lock — e.g. by reintroducing a single `translate`
/// that runs while `inner.lock()` is held — trips this guard.
///
/// We assert on source structure rather than timing because a wall-clock race
/// test would be flaky; the lock-discipline invariant is a property of where
/// the call lives, which is exactly what this pins.
#[test]
fn frame_translation_runs_off_lock_not_inside_apply_prepared() {
    let translate_src = include_str!("../translate.rs");
    let inner_src = include_str!("../inner.rs");

    let prepare = extract_fn_body(translate_src, "fn prepare_event")
        .expect("translate.rs must define prepare_event");
    let apply = extract_fn_body(translate_src, "fn apply_prepared")
        .expect("translate.rs must define apply_prepared");

    assert!(
        prepare.contains("tungstenite_to_relay_frame"),
        "the Message→RelayFrame conversion must happen in the off-lock \
         `prepare_event`; if it moved, the lock-discipline guarantee is lost"
    );
    assert!(
        !apply.contains("tungstenite_to_relay_frame"),
        "`apply_prepared` runs under the PoolInner lock; it must NOT call \
         `tungstenite_to_relay_frame` (the JSON-parsing frame converter) — \
         that would re-introduce Defect 1 (lock held across frame translation)"
    );
    // The translator loop must do the conversion (prepare_event) BEFORE it
    // acquires the lock. Pin that ordering in the loop body.
    let loop_body = extract_fn_body(inner_src, "fn translator_loop")
        .expect("inner.rs must define translator_loop");
    let prepare_at = loop_body
        .find("prepare_event(")
        .expect("translator_loop must call prepare_event");
    let lock_at = loop_body
        .find("inner.lock()")
        .expect("translator_loop must take the inner lock");
    assert!(
        prepare_at < lock_at,
        "translator_loop must call prepare_event (off-lock translation) BEFORE \
         taking inner.lock(); otherwise the parse happens under the lock"
    );
}

/// Return the source text of a function body starting at the `needle`
/// signature, up to the matching closing brace (best-effort brace counting).
fn extract_fn_body<'a>(src: &'a str, needle: &str) -> Option<&'a str> {
    let start = src.find(needle)?;
    let after = &src[start..];
    let open = after.find('{')?;
    let bytes = after.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&after[open..=i]);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn walk_rs_files(dir: &std::path::Path, sink: &mut dyn FnMut(&std::path::Path, &str)) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs_files(&path, sink);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                sink(&path, &contents);
            }
        }
    }
}
