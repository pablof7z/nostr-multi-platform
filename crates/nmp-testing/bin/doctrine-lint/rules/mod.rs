//! Per-rule lint modules. Each rule exposes a `check(line) -> Vec<(col, msg,
//! suggested)>` function that the driver calls per scanned line, and an
//! `ID: &'static str` constant for `// doctrine-allow:` matching.

pub mod a6;
pub mod action_namespace;
pub mod d0;
pub mod d10;
pub mod d11;
pub mod d12;
pub mod d13;
pub mod d14;
pub mod d15;
pub mod d16;
pub mod d17;
pub mod d18;
pub mod d19;
pub mod d20;
pub mod d21;
pub mod d23;
pub mod d24;
pub mod d25;
pub mod d26;
pub mod d27;
pub mod d6;
pub mod d7;
pub mod d8;
pub mod d9;
pub mod no_raw_tap_reintroduction;
pub mod split_call;
