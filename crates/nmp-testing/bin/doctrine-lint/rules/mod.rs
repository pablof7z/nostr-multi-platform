//! Per-rule lint modules. Each rule exposes a `check(line) -> Vec<(col, msg,
//! suggested)>` function that the driver calls per scanned line, and an
//! `ID: &'static str` constant for `// doctrine-allow:` matching.

pub mod d0;
pub mod d6;
pub mod d7;
pub mod d8;
pub mod d9;
