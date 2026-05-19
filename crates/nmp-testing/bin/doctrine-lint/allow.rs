//! Per-line `// doctrine-allow: Dn — reason` opt-out parser.
//!
//! Rules consult this to suppress a finding on a specific line when the
//! author has explicitly justified it. The annotation must appear on the
//! *same line* as the offending code (a trailing comment) — keeps the
//! grep trivial and the audit trail co-located with the exempted line.
//!
//! Shape:
//!
//! ```text
//!     foo.bar.unwrap(); // doctrine-allow: D6 — Mutex poisoning is fatal here
//! ```
//!
//! Multiple rules can be allowed at once: `doctrine-allow: D6,D8 — reason`.

pub fn line_allows(line: &str, rule: &str) -> bool {
    let Some(after) = line.split("// doctrine-allow:").nth(1) else {
        return false;
    };
    // Take everything up to the first separator that signals the reason:
    //   - em-dash `—` (preferred)
    //   - hyphen ` - ` (ASCII fallback)
    //   - any whitespace after the rule token
    // Each entry is a comma-separated rule id; the prose afterwards is the
    // human reason.
    let head = after
        .split('—')
        .next()
        .and_then(|s| s.split(" - ").next())
        .unwrap_or(after);
    head.split(',').any(|r| {
        // Each comma-separated chunk's first whitespace-delimited token
        // is the rule id (everything after the first space is reason prose
        // when the human omits the dash).
        r.split_whitespace().next().map(|t| t == rule).unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::line_allows;

    #[test]
    fn single_rule_allow() {
        let line = "    x.unwrap(); // doctrine-allow: D6 — lock poisoning";
        assert!(line_allows(line, "D6"));
        assert!(!line_allows(line, "D8"));
    }

    #[test]
    fn multi_rule_allow() {
        let line = "    let v = Vec::new(); // doctrine-allow: D6,D8 — bench setup";
        assert!(line_allows(line, "D6"));
        assert!(line_allows(line, "D8"));
        assert!(!line_allows(line, "D7"));
    }

    #[test]
    fn no_annotation_means_no_allow() {
        assert!(!line_allows("    x.unwrap();", "D6"));
    }

    #[test]
    fn allow_without_em_dash_still_works() {
        let line = "x.unwrap(); // doctrine-allow: D6 lock poisoning";
        assert!(line_allows(line, "D6"));
    }
}
