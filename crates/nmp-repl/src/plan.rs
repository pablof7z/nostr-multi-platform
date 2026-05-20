//! Variable expansion + `LogicalInterest` construction.
//!
//! This module **no longer compiles or selects**. The manual
//! `SubscriptionCompiler::with_relays(...).compile(...) + apply_selection(...)`
//! pipeline has been deleted — `req.rs` drives the production
//! [`nmp_core::subs::SubscriptionLifecycle`], whose `recompile_and_diff` IS
//! the outbox (discovery + compile + dead-relay filter + selection, all
//! internal). What remains here is pure variable resolution: turning a
//! parsed `FilterAst` plus expanded `$follows` / literal authors into one
//! concrete `LogicalInterest` the lifecycle can consume.

use std::collections::BTreeSet;

use nmp_core::nip19::decode_npub;
use nmp_core::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, MailboxCache,
};

use crate::ast::{FilterAst, Value};
use crate::error::{ReplError, Result};
use crate::session::Session;

/// Does this filter reference `$follows` anywhere it matters?
pub fn needs_follows(filter: &FilterAst) -> bool {
    let mut nf = false;
    let mut scan = |vals: &[Value]| {
        for v in vals {
            if let Value::Var(name) = v {
                if name == "follows" {
                    nf = true;
                }
            }
        }
    };
    if let Some(a) = &filter.authors {
        scan(a);
    }
    if let Some(i) = &filter.ids {
        scan(i);
    }
    for vals in filter.tags.values() {
        scan(vals);
    }
    nf
}

/// Build the single `LogicalInterest` for this `req` from the parsed filter
/// and the already-resolved `$follows` set. Pure: no I/O. The lifecycle
/// resolves mailboxes / relays itself — this only assembles the *interest*.
///
/// `lifecycle: Tailing` mirrors what a real following-feed ViewModule
/// registers: a tailing subscription kept alive past EOSE. (The REPL's
/// fanout still terminates on EOSE or wall — that is a transport-side
/// decision, independent of the interest's declared lifecycle.)
pub fn build_interest(
    session: &Session,
    filter: &FilterAst,
    follows: &BTreeSet<String>,
) -> Result<LogicalInterest> {
    // Authors: explicit literals (hex / npub) ∪ $-var expansions.
    let mut authors: BTreeSet<String> = BTreeSet::new();
    if let Some(list) = &filter.authors {
        for v in list {
            match v {
                Value::Lit(s) => {
                    authors.insert(expand_literal_author(s)?);
                }
                Value::Var(name) => {
                    for e in expand_var_to_strings(session, name, follows)? {
                        authors.insert(e);
                    }
                }
            }
        }
    }

    // Event ids: literals only (variables unsupported in `ids`).
    let mut event_ids: BTreeSet<String> = BTreeSet::new();
    if let Some(list) = &filter.ids {
        for v in list {
            match v {
                Value::Lit(s) => {
                    if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err(ReplError::Parse(format!(
                            "ids='{s}' — expected 64-char hex event id"
                        )));
                    }
                    event_ids.insert(s.to_lowercase());
                }
                Value::Var(name) => {
                    return Err(ReplError::Variable(format!(
                        "ids=${name} — variables not supported in `ids` field"
                    )));
                }
            }
        }
    }

    let mut shape = InterestShape {
        authors: authors.into_iter().collect(),
        event_ids: event_ids.into_iter().collect(),
        ..Default::default()
    };
    if let Some(kinds) = &filter.kinds {
        shape.kinds = kinds.iter().copied().collect();
    }
    for (letter, values) in &filter.tags {
        let entry = shape.tags.entry(letter.to_string()).or_default();
        for v in values {
            match v {
                Value::Lit(s) => {
                    entry.insert(s.clone());
                }
                Value::Var(name) => {
                    for e in expand_var_to_strings(session, name, follows)? {
                        entry.insert(e);
                    }
                }
            }
        }
    }
    if let Some(since) = filter.since {
        if since >= 0 {
            shape.since = Some(since as u64);
        }
    }
    if let Some(until) = filter.until {
        if until >= 0 {
            shape.until = Some(until as u64);
        }
    }
    shape.limit = filter.limit;

    Ok(LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    })
}

/// Expand a literal author token (hex or npub) to a 64-hex pubkey.
fn expand_literal_author(s: &str) -> Result<String> {
    if s.starts_with("npub1") {
        decode_npub(s)
            .map(|h| h.to_lowercase())
            .map_err(|e| ReplError::Parse(format!("invalid npub '{s}': {e:?}")))
    } else if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(s.to_lowercase())
    } else {
        Err(ReplError::Parse(format!(
            "author '{s}' — expected 64-hex or 'npub1…'"
        )))
    }
}

/// Expand a `$variable` reference into a list of string values appropriate
/// to the surrounding filter field.
///
/// `$relays` / `$inbox` read the seed's kind:10002 from the lifecycle's
/// mailbox cache (populated by a prior `req`'s discovery, or an explicit
/// `req kinds=10002 authors=$me`). This is variable resolution layered on
/// the same cache the lifecycle owns — it does not run the outbox.
pub fn expand_var_to_strings(
    session: &Session,
    name: &str,
    follows: &BTreeSet<String>,
) -> Result<Vec<String>> {
    match name {
        "me" | "seed" => {
            let h = session.seed_hex.as_ref().ok_or_else(|| {
                ReplError::Variable(format!(
                    "${name} requires a seed; run `set-seed <nip05|npub>` first"
                ))
            })?;
            Ok(vec![h.clone()])
        }
        "follows" => {
            if !follows.is_empty() {
                Ok(follows.iter().cloned().collect())
            } else if let Some(cached) = &session.follows_cache {
                Ok(cached.iter().cloned().collect())
            } else {
                Err(ReplError::Variable(
                    "$follows requires a seed; run `set-seed <nip05|npub>` first".to_string(),
                ))
            }
        }
        "relays" | "inbox" => {
            let hex = session.seed_hex.as_ref().ok_or_else(|| {
                ReplError::Variable(format!(
                    "${name} requires a seed; run `set-seed <nip05|npub>` first"
                ))
            })?;
            let snap = session.mailbox_cache.get(hex).ok_or_else(|| {
                ReplError::Variable(format!(
                    "${name}: seed has no cached kind:10002 mailbox; run `req kinds=10002 authors=$me` first"
                ))
            })?;
            let mut out: BTreeSet<String> = BTreeSet::new();
            if name == "inbox" {
                for u in &snap.read_relays {
                    out.insert(u.clone());
                }
            } else {
                for u in &snap.write_relays {
                    out.insert(u.clone());
                }
            }
            for u in &snap.both_relays {
                out.insert(u.clone());
            }
            Ok(out.into_iter().collect())
        }
        other => Err(ReplError::Variable(format!(
            "${other} is not a known variable (try $me, $seed, $follows, $relays, $inbox)"
        ))),
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Value as AstValue;

    const HEX64_A: &str =
        "fa984bd7dbb282f07e16e7ae87b26a2a7b9b9077b8a5d6c10d3c84d54f76d2a1";
    const HEX64_B: &str =
        "0000000000000000000000000000000000000000000000000000000000000001";

    // ── needs_follows ────────────────────────────────────────────────────

    #[test]
    fn needs_follows_true_when_authors_reference_follows() {
        let mut f = FilterAst::default();
        f.authors = Some(vec![AstValue::Var("follows".to_string())]);
        assert!(needs_follows(&f));
    }

    #[test]
    fn needs_follows_false_for_other_vars() {
        let mut f = FilterAst::default();
        f.authors = Some(vec![AstValue::Var("me".to_string())]);
        assert!(!needs_follows(&f));
    }

    #[test]
    fn needs_follows_false_for_literal_only() {
        let mut f = FilterAst::default();
        f.authors = Some(vec![AstValue::Lit(HEX64_A.to_string())]);
        assert!(!needs_follows(&f));
    }

    #[test]
    fn needs_follows_scans_tag_values() {
        let mut f = FilterAst::default();
        f.tags
            .insert('p', vec![AstValue::Var("follows".to_string())]);
        assert!(needs_follows(&f));
    }

    #[test]
    fn needs_follows_false_for_empty_filter() {
        assert!(!needs_follows(&FilterAst::default()));
    }

    // ── expand_literal_author ────────────────────────────────────────────

    #[test]
    fn expand_literal_author_accepts_hex() {
        assert_eq!(expand_literal_author(HEX64_A).unwrap(), HEX64_A);
    }

    #[test]
    fn expand_literal_author_lowercases_hex() {
        let upper = HEX64_A.to_uppercase();
        assert_eq!(expand_literal_author(&upper).unwrap(), HEX64_A);
    }

    #[test]
    fn expand_literal_author_rejects_short_token() {
        let err = expand_literal_author("deadbeef").unwrap_err();
        assert!(matches!(err, ReplError::Parse(_)));
    }

    #[test]
    fn expand_literal_author_rejects_non_hex_64_chars() {
        // 64 chars but contains a non-hex char.
        let bad = "z".repeat(64);
        assert!(expand_literal_author(&bad).is_err());
    }

    // ── build_interest ───────────────────────────────────────────────────

    #[test]
    fn build_interest_assembles_kinds_and_literal_authors() {
        let session = Session::default();
        let mut f = FilterAst::default();
        f.kinds = Some(vec![1, 6]);
        f.authors = Some(vec![AstValue::Lit(HEX64_A.to_string())]);
        let interest = build_interest(&session, &f, &BTreeSet::new()).unwrap();
        assert!(interest.shape.kinds.contains(&1));
        assert!(interest.shape.kinds.contains(&6));
        assert!(interest.shape.authors.contains(&HEX64_A.to_string()));
    }

    #[test]
    fn build_interest_expands_follows_var_into_authors() {
        let session = Session::default();
        let mut f = FilterAst::default();
        f.kinds = Some(vec![1]);
        f.authors = Some(vec![AstValue::Var("follows".to_string())]);
        let mut follows = BTreeSet::new();
        follows.insert(HEX64_A.to_string());
        follows.insert(HEX64_B.to_string());
        let interest = build_interest(&session, &f, &follows).unwrap();
        assert_eq!(interest.shape.authors.len(), 2);
        assert!(interest.shape.authors.contains(&HEX64_A.to_string()));
        assert!(interest.shape.authors.contains(&HEX64_B.to_string()));
    }

    #[test]
    fn build_interest_rejects_bad_event_id() {
        let session = Session::default();
        let mut f = FilterAst::default();
        f.ids = Some(vec![AstValue::Lit("tooshort".to_string())]);
        let err = build_interest(&session, &f, &BTreeSet::new()).unwrap_err();
        assert!(matches!(err, ReplError::Parse(_)));
    }

    #[test]
    fn build_interest_rejects_var_in_ids_field() {
        let session = Session::default();
        let mut f = FilterAst::default();
        f.ids = Some(vec![AstValue::Var("follows".to_string())]);
        let err = build_interest(&session, &f, &BTreeSet::new()).unwrap_err();
        assert!(matches!(err, ReplError::Variable(_)));
    }

    #[test]
    fn build_interest_drops_negative_timestamps() {
        let session = Session::default();
        let mut f = FilterAst::default();
        f.kinds = Some(vec![1]);
        f.since = Some(-5);
        f.until = Some(1_700_000_000);
        let interest = build_interest(&session, &f, &BTreeSet::new()).unwrap();
        assert!(interest.shape.since.is_none(), "negative since is dropped");
        assert_eq!(interest.shape.until, Some(1_700_000_000));
    }

    #[test]
    fn build_interest_lifecycle_is_tailing() {
        let session = Session::default();
        let mut f = FilterAst::default();
        f.kinds = Some(vec![1]);
        let interest = build_interest(&session, &f, &BTreeSet::new()).unwrap();
        assert!(matches!(
            interest.lifecycle,
            nmp_core::planner::InterestLifecycle::Tailing
        ));
    }

    // ── expand_var_to_strings ────────────────────────────────────────────

    #[test]
    fn expand_var_me_requires_seed() {
        let session = Session::default();
        let err =
            expand_var_to_strings(&session, "me", &BTreeSet::new()).unwrap_err();
        assert!(matches!(err, ReplError::Variable(_)));
    }

    #[test]
    fn expand_var_me_returns_seed_when_set() {
        let mut session = Session::default();
        session.seed_hex = Some(HEX64_A.to_string());
        let out = expand_var_to_strings(&session, "me", &BTreeSet::new()).unwrap();
        assert_eq!(out, vec![HEX64_A.to_string()]);
        // `seed` is an alias for `me`.
        let out2 =
            expand_var_to_strings(&session, "seed", &BTreeSet::new()).unwrap();
        assert_eq!(out2, vec![HEX64_A.to_string()]);
    }

    #[test]
    fn expand_var_follows_uses_passed_set_first() {
        let session = Session::default();
        let mut follows = BTreeSet::new();
        follows.insert(HEX64_A.to_string());
        let out = expand_var_to_strings(&session, "follows", &follows).unwrap();
        assert_eq!(out, vec![HEX64_A.to_string()]);
    }

    #[test]
    fn expand_var_follows_falls_back_to_cache() {
        let mut session = Session::default();
        let mut cached = BTreeSet::new();
        cached.insert(HEX64_B.to_string());
        session.follows_cache = Some(cached);
        // Empty passed-in set → fall back to the session's follows_cache.
        let out =
            expand_var_to_strings(&session, "follows", &BTreeSet::new()).unwrap();
        assert_eq!(out, vec![HEX64_B.to_string()]);
    }

    #[test]
    fn expand_var_follows_errors_when_unset() {
        let session = Session::default();
        let err = expand_var_to_strings(&session, "follows", &BTreeSet::new())
            .unwrap_err();
        assert!(matches!(err, ReplError::Variable(_)));
    }

    #[test]
    fn expand_var_unknown_name_errors() {
        let session = Session::default();
        let err = expand_var_to_strings(&session, "bogus", &BTreeSet::new())
            .unwrap_err();
        match err {
            ReplError::Variable(msg) => {
                assert!(msg.contains("not a known variable"), "got {msg}")
            }
            other => panic!("expected Variable error, got {other:?}"),
        }
    }

    #[test]
    fn expand_var_relays_requires_seed() {
        let session = Session::default();
        let err = expand_var_to_strings(&session, "relays", &BTreeSet::new())
            .unwrap_err();
        assert!(matches!(err, ReplError::Variable(_)));
    }
}
