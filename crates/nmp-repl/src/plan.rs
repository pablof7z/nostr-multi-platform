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
