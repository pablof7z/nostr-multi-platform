//! Variable expansion + compiler wiring. Translates a parsed `FilterAst`
//! into a `LogicalInterest`, runs `SubscriptionCompiler::with_relays` +
//! `apply_selection`, and returns the relay→authors map.
//!
//! Phase A and B discovery is driven from here on-demand: any `$follows` or
//! `$relays` reference triggers the lookup if its cache is empty.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use nmp_core::nip19::decode_npub;
use nmp_core::planner::{
    apply_selection, InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope,
    InterestShape, LogicalInterest, SubscriptionCompiler,
};

use crate::ast::{FilterAst, Value};
use crate::discovery::{phase_a, phase_b};
use crate::error::{ReplError, Result};
use crate::session::Session;

/// Per-phase timings + plan summary for the renderer.
pub struct PreparedPlan {
    pub per_relay_authors: BTreeMap<String, Vec<String>>,
    pub naive_relays: usize,
    pub authors_on_wire: usize,
    pub unroutable: usize,
    pub follows_used: bool,
    pub phase_a_elapsed: Duration,
    pub phase_a_cached: bool,
    pub phase_a_count: usize,
    pub phase_b_elapsed: Duration,
    pub phase_b_fetched: usize,
    pub phase_b_cached: usize,
    /// Total authors a kind:10002 was needed for.
    pub phase_b_queried: usize,
    /// How many of those have a mailbox after phase B (the gap vs.
    /// `phase_b_queried` is the unroutable surface — §13.8).
    pub phase_b_have: usize,
    pub phase_c_elapsed: Duration,
    /// Filter shape echoed back so the per-relay REQ can use it.
    pub filter: FilterAst,
}

/// Build a `PreparedPlan` from the parsed filter, running phases A/B/C in
/// order. The renderer prints status between phases — this function does
/// not print; it returns timings.
pub fn prepare(session: &mut Session, filter: &FilterAst) -> Result<PreparedPlan> {
    // ── Variable scan ────────────────────────────────────────────────────
    let mut needs_follows = false;
    let mut needs_relays = false;
    scan_vars(
        filter,
        &mut needs_follows,
        &mut needs_relays,
    );

    // Static authors (literals from the filter, expanded npubs).
    let mut explicit_authors: BTreeSet<String> = BTreeSet::new();
    if let Some(authors) = &filter.authors {
        for v in authors {
            if let Value::Lit(s) = v {
                explicit_authors.insert(expand_literal_author(s)?);
            }
        }
    }
    let mut explicit_ids: BTreeSet<String> = BTreeSet::new();
    if let Some(ids) = &filter.ids {
        for v in ids {
            match v {
                Value::Lit(s) => {
                    if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err(ReplError::Parse(format!(
                            "ids='{s}' — expected 64-char hex event id"
                        )));
                    }
                    explicit_ids.insert(s.to_lowercase());
                }
                Value::Var(name) => {
                    return Err(ReplError::Variable(format!(
                        "ids=$ {name} — variables not supported in `ids` field"
                    )));
                }
            }
        }
    }

    // ── Phase A: follows (if needed) ─────────────────────────────────────
    let mut phase_a_elapsed = Duration::ZERO;
    let mut phase_a_cached = false;
    let mut phase_a_count = 0usize;
    let mut follows: BTreeSet<String> = BTreeSet::new();
    if needs_follows {
        let a = phase_a(session)?;
        phase_a_elapsed = a.elapsed;
        phase_a_cached = a.cached;
        phase_a_count = a.follows.len();
        follows = a.follows;
    }

    // ── Tags + ids: scan for $vars (currently only $me / $seed / $follows
    // are meaningful — others are unsupported) ───────────────────────────
    let mut tags_expanded: BTreeSet<(String, String)> = BTreeSet::new();
    for (letter, values) in &filter.tags {
        for v in values {
            match v {
                Value::Lit(s) => {
                    tags_expanded.insert((letter.to_string(), s.clone()));
                }
                Value::Var(name) => {
                    let expansions = expand_var_to_strings(session, name, &follows)?;
                    for e in expansions {
                        tags_expanded.insert((letter.to_string(), e));
                    }
                }
            }
        }
    }

    // Authors: union explicit + $-vars expansions.
    let mut authors_final: BTreeSet<String> = explicit_authors.clone();
    if let Some(authors) = &filter.authors {
        for v in authors {
            if let Value::Var(name) = v {
                let expansions = expand_var_to_strings(session, name, &follows)?;
                for e in expansions {
                    authors_final.insert(e);
                }
            }
        }
    }

    // ── Phase B: mailboxes for every author we care about ────────────────
    // We need kind:10002 for: (a) every author in the filter, (b) anyone
    // referenced via $follows.
    let mut mailbox_targets: BTreeSet<String> = authors_final.clone();
    if needs_follows {
        for pk in &follows {
            mailbox_targets.insert(pk.clone());
        }
    }
    let _ = needs_relays; // referenced; mailbox lookup is implicit for follows authors.
    let mut phase_b_elapsed = Duration::ZERO;
    let mut phase_b_fetched = 0usize;
    let mut phase_b_cached = 0usize;
    let mut phase_b_queried = 0usize;
    let mut phase_b_have = 0usize;
    if !mailbox_targets.is_empty() {
        let b = phase_b(session, &mailbox_targets)?;
        phase_b_elapsed = b.elapsed;
        phase_b_fetched = b.fetched;
        phase_b_cached = b.already_cached;
        phase_b_queried = b.queried;
        phase_b_have = b.have_after;
    }

    // ── Phase C: compile + select ────────────────────────────────────────
    let phase_c_start = std::time::Instant::now();
    // Build the InterestShape.
    let mut shape = InterestShape::default();
    shape.authors = authors_final.iter().cloned().collect();
    if let Some(kinds) = &filter.kinds {
        shape.kinds = kinds.iter().copied().collect();
    }
    for (letter, value) in &tags_expanded {
        shape
            .tags
            .entry(letter.clone())
            .or_default()
            .insert(value.clone());
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
    shape.event_ids = explicit_ids.iter().cloned().collect();

    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
    };

    let mut cache = InMemoryMailboxCache::new();
    for (pk, snap) in &session.mailbox_cache {
        cache.put(pk.clone(), snap.clone());
    }
    let empty: Vec<String> = Vec::new();
    let compiler = SubscriptionCompiler::with_relays(
        &cache,
        &session.indexer_relays,
        &empty,
        &session.app_relays,
    );
    let mut plan = compiler
        .compile(&[interest])
        .map_err(|e| ReplError::Planner(format!("{e:?}")))?;
    let unroutable = plan.unroutable_authors.len();
    let naive_relays = plan.per_relay.len();
    apply_selection(&mut plan, session.max_connections, session.max_per_user);

    // Strip dead relays from the optimised set.
    if !session.dead_relays.is_empty() {
        plan.per_relay.retain(|url, _| !session.dead_relays.contains(url));
    }

    let mut per_relay_authors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (relay_url, rp) in &plan.per_relay {
        let mut authors: BTreeSet<String> = BTreeSet::new();
        for sub in &rp.sub_shapes {
            for author in &sub.shape.authors {
                authors.insert(author.clone());
            }
        }
        per_relay_authors.insert(relay_url.clone(), authors.into_iter().collect());
    }

    let authors_on_wire: usize = per_relay_authors.values().map(|v| v.len()).sum();
    let phase_c_elapsed = phase_c_start.elapsed();

    Ok(PreparedPlan {
        per_relay_authors,
        naive_relays,
        authors_on_wire,
        unroutable,
        follows_used: needs_follows,
        phase_a_elapsed,
        phase_a_cached,
        phase_a_count,
        phase_b_elapsed,
        phase_b_fetched,
        phase_b_cached,
        phase_b_queried,
        phase_b_have,
        phase_c_elapsed,
        filter: filter.clone(),
    })
}

fn scan_vars(filter: &FilterAst, needs_follows: &mut bool, needs_relays: &mut bool) {
    let scan_list = |vals: &[Value], nf: &mut bool, nr: &mut bool| {
        for v in vals {
            if let Value::Var(name) = v {
                match name.as_str() {
                    "follows" => *nf = true,
                    "relays" | "inbox" => *nr = true,
                    _ => {}
                }
            }
        }
    };
    if let Some(a) = &filter.authors {
        scan_list(a, needs_follows, needs_relays);
    }
    if let Some(i) = &filter.ids {
        scan_list(i, needs_follows, needs_relays);
    }
    for vals in filter.tags.values() {
        scan_list(vals, needs_follows, needs_relays);
    }
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
pub fn expand_var_to_strings(
    session: &Session,
    name: &str,
    follows_phase_a: &BTreeSet<String>,
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
            if follows_phase_a.is_empty() {
                // Was not pre-fetched (caller forgot to scan); take from cache
                // if we have it, else error.
                let cached = session.follows_cache.as_ref().ok_or_else(|| {
                    ReplError::Variable(
                        "$follows requires a seed; run `set-seed <nip05|npub>` first".to_string(),
                    )
                })?;
                Ok(cached.iter().cloned().collect())
            } else {
                Ok(follows_phase_a.iter().cloned().collect())
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
                    "${name}: seed has no cached kind:10002 mailbox; try a `req kinds=1 authors=$me` first"
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
            "$ {other} is not a known variable (try $me, $seed, $follows, $relays, $inbox)"
        ))),
    }
}
