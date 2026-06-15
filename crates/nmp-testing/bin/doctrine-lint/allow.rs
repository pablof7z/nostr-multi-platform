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
        r.split_whitespace()
            .next()
            .map(|t| t == rule)
            .unwrap_or(false)
    })
}

/// Reason-REQUIRED variant of [`line_allows`] (the D10/D21 tightened idiom).
///
/// A bare `// doctrine-allow: DNN` (no prose after a `—` / ` - ` separator) does
/// NOT silence the finding — every escape must carry an auditable justification.
/// Used by the event-flow gates D23/D24/D25 so a reasonless allow is rejected.
pub fn line_allows_with_reason(line: &str, rule: &str) -> bool {
    let Some(after) = line.split("// doctrine-allow:").nth(1) else {
        return false;
    };
    let (head, reason) = if let Some((h, r)) = after.split_once('—') {
        (h, r)
    } else if let Some((h, r)) = after.split_once(" - ") {
        (h, r)
    } else {
        return false;
    };
    if reason.trim().is_empty() {
        return false;
    }
    head.split(',').any(|r| {
        r.split_whitespace()
            .next()
            .map(|t| t == rule)
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::{line_allows, line_allows_with_reason};

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

    #[test]
    fn reason_required_accepts_with_reason() {
        let line = "    store.insert(e); // doctrine-allow: D23 — migration backfill";
        assert!(line_allows_with_reason(line, "D23"));
        assert!(!line_allows_with_reason(line, "D24"));
    }

    #[test]
    fn reason_required_rejects_bare_allow() {
        // No `—`/` - ` separator + prose → not silenced (the tightened idiom).
        let line = "    store.insert(e); // doctrine-allow: D23";
        assert!(!line_allows_with_reason(line, "D23"));
    }

    #[test]
    fn reason_required_rejects_empty_reason() {
        let line = "    store.insert(e); // doctrine-allow: D23 —   ";
        assert!(!line_allows_with_reason(line, "D23"));
    }
}
