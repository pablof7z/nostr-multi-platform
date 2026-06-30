//! Conformance-catalog drift gate.
//!
//! Binds `docs/builder-guide/conformance/catalog.md` (the consumer-facing
//! re-cut of the doctrine consumed by the `nmp-conformance` scanner skill) to
//! the live canon. Modeled on `framework_magic_contract::contract_surface_complete`.
//!
//! The catalog deliberately holds only the *new* consumer-side information
//! (detection signature + severity); the *why* lives in the cited canon. That
//! split is only safe if a gate guarantees every cited `Origin` still maps to a
//! live doctrine / contract bullet — otherwise the catalog silently becomes a
//! parallel (drifting) source of truth, the one thing the repo forbids hardest.
//!
//! This gate **derives** the valid id set from canon rather than hardcoding it,
//! so removing a doctrine/contract from canon (as happened to C10 when
//! `nmp-nip77` was deleted) immediately fails any catalog rule still citing it.
//!
//! Invocation: `cargo test -p nmp-testing --test conformance_catalog_complete`

use std::collections::BTreeSet;

const CATALOG: &str = include_str!("../../../docs/builder-guide/conformance/catalog.md");
const DOCTRINE: &str = include_str!("../../../docs/product-spec/doctrine.md");
const CONTRACT: &str = include_str!("../../../docs/design/framework-magic.md");
const FRESHNESS: &str = include_str!("../../../docs/design/replaceable-freshness.md");

/// Valid doctrine ids — derived from the `## D<n>.` headers in the doctrine canon.
fn valid_doctrine_ids() -> BTreeSet<String> {
    DOCTRINE
        .lines()
        .filter_map(|l| {
            let l = l.trim_start_matches('#').trim();
            // Header form: "D0. The framework core knows nothing ..."
            if let Some(rest) = l.strip_prefix('D') {
                let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !num.is_empty() && rest[num.len()..].starts_with('.') {
                    return Some(format!("D{num}"));
                }
            }
            None
        })
        .collect()
}

/// Valid contract ids — derived from the `| C<n> | ... |` rows of the
/// framework-magic contract table (same source `contract_surface_complete` parses).
fn valid_contract_ids() -> BTreeSet<String> {
    CONTRACT
        .lines()
        .filter(|l| l.trim_start().starts_with("| C"))
        .filter_map(|l| {
            let id = l.trim_start().trim_start_matches('|').trim();
            let id = id.split('|').next()?.trim();
            // id like "C1", "C13"
            if let Some(rest) = id.strip_prefix('C') {
                if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                    return Some(id.to_string());
                }
            }
            None
        })
        .collect()
}

/// Extract `D<n>` / `C<n>` / `F-TTL` tokens from a single `Origin` cell.
fn origin_tokens(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    if cell.contains("F-TTL") {
        out.push("F-TTL".to_string());
    }
    // Walk the cell pulling D<digits> / C<digits> tokens.
    let bytes = cell.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == 'D' || c == 'C' {
            let num: String = cell[i + 1..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !num.is_empty() {
                out.push(format!("{c}{num}"));
                i += 1 + num.len();
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Parse the catalog rule rows -> (rule_id, origin_cell). A rule row is a table
/// line whose first cell is an id like `A1`, `D5`, `L2`.
fn catalog_rules() -> Vec<(String, String)> {
    CATALOG
        .lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .filter_map(|l| {
            let cols: Vec<&str> = l.split('|').collect();
            // | <id> | rule | origin | layer | sev | detection |
            if cols.len() < 7 {
                return None;
            }
            let id = cols[1].trim();
            let is_rule_id = {
                let mut ch = id.chars();
                matches!(ch.next(), Some(c) if c.is_ascii_uppercase())
                    && ch
                        .clone()
                        .next()
                        .map(|c| c.is_ascii_digit())
                        .unwrap_or(false)
                    && id.len() >= 2
            };
            if !is_rule_id {
                return None; // header row / separator row
            }
            Some((id.to_string(), cols[3].trim().to_string()))
        })
        .collect()
}

#[test]
fn conformance_catalog_origins_bind_to_live_canon() {
    let doctrine = valid_doctrine_ids();
    let contract = valid_contract_ids();
    let ftl_valid = FRESHNESS.contains("F-TTL");

    // Parse sanity: canon derivation must not silently yield nothing.
    assert!(
        doctrine.len() >= 10,
        "derived only {} doctrine ids from doctrine.md (expected D0..D10) — parser drift?",
        doctrine.len()
    );
    assert!(
        contract.len() >= 10,
        "derived only {} contract ids from framework-magic.md — parser drift?",
        contract.len()
    );
    assert!(
        ftl_valid,
        "F-TTL no longer found in replaceable-freshness.md canon"
    );

    let mut valid: BTreeSet<String> = BTreeSet::new();
    valid.extend(doctrine.iter().cloned());
    valid.extend(contract.iter().cloned());
    valid.insert("F-TTL".to_string());

    let rules = catalog_rules();
    assert!(
        rules.len() >= 20,
        "parsed only {} catalog rules (expected ~35) — table format drift?",
        rules.len()
    );

    let mut violations: Vec<String> = Vec::new();
    for (id, origin) in &rules {
        let tokens = origin_tokens(origin);
        assert!(
            !tokens.is_empty(),
            "catalog rule {id} has an Origin cell with no D<n>/C<n>/F-TTL token: {origin:?}"
        );
        for tok in tokens {
            if !valid.contains(&tok) {
                violations.push(format!(
                    "rule {id} cites Origin '{tok}' which is not a live canon id \
                     (valid: {:?})",
                    valid
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "conformance catalog cites canon bullets that no longer exist \
         (drift — update catalog.md or restore the canon):\n  {}",
        violations.join("\n  ")
    );
}
