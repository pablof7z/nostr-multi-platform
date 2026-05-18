//! Positive D8 fixture — has a `// hot path`-marked function with banned
//! allocations inside. Smoke test invokes with `--d8-extra-scope
//! fixtures/d8` so the path-scope opens.

pub fn ingest_event(event: &str) {
    // hot path
    let s = format!("ingested: {}", event);
    let v: Vec<u8> = Vec::new();
    let b = Box::new(0u8);
    let _ = (s, v, b);
}
