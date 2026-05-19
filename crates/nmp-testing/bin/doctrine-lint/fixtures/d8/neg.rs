//! Negative D8 fixture — no `// hot path` marker anywhere, so every
//! allocation is by-construction out of scope. Should report zero findings
//! even though the file lives in the d8 in-scope path.
//!
//! Also demonstrates `// doctrine-allow: D8` opt-out on a single line
//! inside a marked function (rare but supported).

pub fn cold_path(event: &str) {
    // No `// hot path` marker → D8 doesn't fire on any of these:
    let s = format!("safely allocated: {}", event);
    let v: Vec<u8> = Vec::new();
    let b = Box::new(0u8);
    let _ = (s, v, b);
}

pub fn ingest_with_explicit_exemption(event: &str) {
    // hot path
    // One-off scratch buffer — author has justified the allocation.
    let s = format!("{}", event); // doctrine-allow: D8 — bench scaffolding only
    let _ = s;
}
