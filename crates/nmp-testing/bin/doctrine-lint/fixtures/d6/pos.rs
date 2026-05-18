//! Positive D6 fixture — must trigger D6 findings.
//!
//! Production-shaped code (no `#[cfg(test)]` gate, not a `tests.rs`-shaped
//! filename) containing the banned patterns.

pub fn risky_path(opt: Option<u8>) -> u8 {
    let x = opt.unwrap();
    panic!("unreachable in theory");
    let _ = x;
    todo!()
}

pub fn assert_invariant(maybe: Option<String>) -> String {
    maybe.expect("must be set")
}
