//! The subscription compiler: 4-stage pipeline from `Vec<LogicalInterest>`
//! to `CompiledPlan`.
//!
//! ## Pipeline stages
//!
//! 1. **Resolve authors → mailboxes** — consult `MailboxCache` (stubbed in
//!    phase 1 via `EmptyMailboxCache`; real impl lives in `nmp-nip65`).
//! 2. **Indexer fallback** — authors with no known mailbox route to the
//!    configured indexer set.
//! 3. **Per-relay shape merge** — group by relay URL; merge compatible shapes
//!    with `lattice::merge()` (Rules 1–8). Author sets are partitioned per
//!    relay — only authors that declared a relay appear in its sub-shape.
//! 4. **Plan-id binding** — hash(sorted interests, sorted mailbox snapshot,
//!    lattice version) → stable `plan_id`.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3
//! Doctrine: D3 (outbox routing automatic), D6 (errors never cross FFI),
//!           D8 (zero per-event allocs after warmup).
//!
//! Phase 1 scope: mailbox stage stubs to `EmptyMailboxCache`. Real mailbox
//! resolution lives in `nmp-nip65` (separate later slice). The call-point
//! for `EventStore::coverage()` is marked with a placeholder comment per the
//! task spec (not wired in phase 1).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{
    interest::{
        InterestId, InterestLifecycle, InterestShape, LogicalInterest, NaddrCoord, Pubkey,
        RelayUrl,
    },
    lattice::{merge, MergeOutcome},
    plan::{CompiledPlan, PlannerError, RelayPlan, RoutingSource, SubShape},
};

// ─── MailboxCache seam ───────────────────────────────────────────────────────

/// Minimal mailbox snapshot used by the compiler.
///
/// Phase 1: only `write_relays` and `both_relays` are consumed (Outbox
/// direction). Inbox direction (read_relays) is used for `#p` interests, which
/// are not yet wired in phase 1.
///
/// Full trait lives in `nmp-nip65::cache::MailboxCache` (later slice).
#[derive(Clone, Debug, Default)]
pub struct MailboxSnapshot {
    pub write_relays: Vec<RelayUrl>,
    pub read_relays: Vec<RelayUrl>,
    pub both_relays: Vec<RelayUrl>,
}

impl MailboxSnapshot {
    /// All relays relevant for Outbox direction (write + both).
    pub fn outbox_relays(&self) -> impl Iterator<Item = &RelayUrl> {
        self.write_relays.iter().chain(self.both_relays.iter())
    }
}

/// Minimum surface the compiler needs for mailbox lookups.
/// Phase 1 implementation: `EmptyMailboxCache` always returns `None`.
/// Phase 2 implementation: `nmp-nip65::InMemoryMailboxCache`.
pub trait MailboxCache: Send + Sync {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot>;
    /// Snapshot of all known entries for plan-id hashing.
    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)>;
    /// Monotonic generation counter — advances on every accepted `put`.
    fn generation(&self) -> u64;
}

/// Phase 1 stub: no mailbox data. All authors fall back to the indexer set.
pub struct EmptyMailboxCache;

impl MailboxCache for EmptyMailboxCache {
    fn get(&self, _pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        None
    }
    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
        Vec::new()
    }
    fn generation(&self) -> u64 {
        0
    }
}

/// Simple in-memory mailbox cache for tests and the planner harness.
#[derive(Default)]
pub struct InMemoryMailboxCache {
    data: HashMap<Pubkey, MailboxSnapshot>,
    generation: u64,
}

impl InMemoryMailboxCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, pubkey: Pubkey, snapshot: MailboxSnapshot) {
        self.data.insert(pubkey, snapshot);
        self.generation = self.generation.saturating_add(1);
    }
}

impl MailboxCache for InMemoryMailboxCache {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        self.data.get(pubkey).cloned()
    }
    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
        self.data.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
    fn generation(&self) -> u64 {
        self.generation
    }
}

// ─── Internal relay-partitioned entry ────────────────────────────────────────

/// A relay-partitioned slice of one logical interest.
///
/// When an interest has N authors, Stage 1 produces one `RelayEntry` per
/// `(relay, interest_id)` pair, where `authors_for_relay` contains only the
/// authors that declared this specific relay (not all N authors). This is the
/// author-partitioning that lets the merge lattice produce per-relay author
/// subsets.
struct RelayEntry {
    /// The interest's non-author fields (kinds, tags, since, until, etc.).
    /// `authors` is intentionally left empty here; we merge `authors_for_relay`
    /// in at Stage 3 merge time.
    base_shape: InterestShape,
    /// The subset of authors from this interest that declared this relay.
    authors_for_relay: BTreeSet<Pubkey>,
    /// Address-pointer coordinates from this interest (if relevant for routing).
    addresses_for_relay: BTreeSet<NaddrCoord>,
    lifecycle: InterestLifecycle,
    source: RoutingSource,
    interest_id: InterestId,
}

impl RelayEntry {
    /// Construct the final `InterestShape` for this relay slice.
    fn into_shape(mut self) -> (InterestShape, InterestLifecycle, RoutingSource, InterestId) {
        self.base_shape.authors = self.authors_for_relay;
        self.base_shape.addresses = self.addresses_for_relay;
        (self.base_shape, self.lifecycle, self.source, self.interest_id)
    }
}

// ─── SubscriptionCompiler ────────────────────────────────────────────────────

/// Version of the merge lattice — bump when Rule semantics change.
/// Included in plan-id hash to ensure plan-ids invalidate on lattice changes.
const MERGE_LATTICE_VERSION: u8 = 1;

/// The subscription compiler.
///
/// Holds a reference to the mailbox cache and indexer relay set. Both may be
/// updated between compilations (the compiler always reads the current state).
pub struct SubscriptionCompiler<'a> {
    mailbox_cache: &'a dyn MailboxCache,
    indexer_relays: &'a [RelayUrl],
}

impl<'a> SubscriptionCompiler<'a> {
    /// Construct a compiler bound to a mailbox cache and indexer set.
    pub fn new(mailbox_cache: &'a dyn MailboxCache, indexer_relays: &'a [RelayUrl]) -> Self {
        Self { mailbox_cache, indexer_relays }
    }

    /// Compile a set of logical interests into a `CompiledPlan`.
    ///
    /// ## Stages
    /// 1. Resolve each interest's authors to relay URLs via the mailbox cache.
    ///    Authors are **partitioned** per relay — only the authors that declared
    ///    a relay appear in that relay's sub-shape.
    /// 2. Fall back missing authors to the indexer set.
    /// 3. Group relay entries by relay URL; merge compatible shapes per Rules 1–8.
    /// 4. Compute `plan_id` via a stable deterministic hash.
    ///
    /// # EventStore coverage (phase 1 placeholder)
    /// The compiler does not yet consult `EventStore::coverage()` for cache-aware
    /// planning. The call-point is reserved here for the phase 2 / M3 wiring:
    ///
    /// ```text
    /// // TODO(phase2): let coverage = event_store.coverage(&watermark_key)?;
    /// // Use coverage to skip REQs whose time-range is fully cached locally.
    /// ```
    pub fn compile(
        &self,
        interests: &[LogicalInterest],
    ) -> Result<CompiledPlan, PlannerError> {
        // ── Stages 1 + 2: author-partitioned relay entry collection ──────────
        // relay_url → Vec<RelayEntry>
        let mut relay_entries: BTreeMap<RelayUrl, Vec<RelayEntry>> = BTreeMap::new();

        for interest in interests {
            self.partition_interest(interest, &mut relay_entries);
        }

        // ── Stage 3: Per-relay shape merge ──────────────────────────────────
        let mut per_relay: BTreeMap<RelayUrl, RelayPlan> = BTreeMap::new();

        for (relay_url, entries) in relay_entries {
            let mut role_tags: BTreeSet<RoutingSource> = BTreeSet::new();

            // Convert RelayEntry → (shape, lifecycle, source, id)
            let mut resolved: Vec<(InterestShape, InterestLifecycle, RoutingSource, InterestId)> =
                entries
                    .into_iter()
                    .map(|entry| {
                        let source = entry.source.clone();
                        role_tags.insert(source);
                        entry.into_shape()
                    })
                    .collect();

            // Greedy pairwise merge
            let mut sub_shapes: Vec<(InterestShape, InterestLifecycle, Vec<InterestId>)> =
                Vec::new();

            for (shape, lifecycle, _source, interest_id) in resolved.drain(..) {
                let mut merged = false;
                for (existing_shape, existing_lifecycle, existing_ids) in sub_shapes.iter_mut() {
                    if let MergeOutcome::Merged(new_shape) =
                        merge(&existing_shape.clone(), &shape, existing_lifecycle, &lifecycle)
                    {
                        *existing_shape = new_shape;
                        existing_ids.push(interest_id.clone());
                        merged = true;
                        break;
                    }
                }
                if !merged {
                    sub_shapes.push((shape, lifecycle, vec![interest_id]));
                }
            }

            let relay_sub_shapes: Vec<SubShape> = sub_shapes
                .into_iter()
                .map(|(shape, _lifecycle, ids)| {
                    let hash = simple_shape_hash(&shape);
                    SubShape {
                        shape,
                        originating_interests: ids,
                        canonical_filter_hash: hash,
                    }
                })
                .collect();

            per_relay.insert(
                relay_url.clone(),
                RelayPlan {
                    relay_url,
                    role_tags,
                    sub_shapes: relay_sub_shapes,
                },
            );
        }

        // ── Stage 4: Plan-id binding ──────────────────────────────────────────
        let plan_id = compute_plan_id(interests, self.mailbox_cache, MERGE_LATTICE_VERSION);

        Ok(CompiledPlan { plan_id, per_relay })
    }

    /// Stage 1 + 2: partition one logical interest into per-relay entries.
    ///
    /// Each entry carries only the AUTHORS that declared the specific relay,
    /// preserving the per-relay author-subset semantics required by the audit
    /// test (Assertion 2) and the design spec §3.3.
    fn partition_interest(
        &self,
        interest: &LogicalInterest,
        relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
    ) {
        // Base shape: everything except authors and addresses (will be filled per relay).
        let base_shape = InterestShape {
            authors: BTreeSet::new(),
            kinds: interest.shape.kinds.clone(),
            tags: interest.shape.tags.clone(),
            since: interest.shape.since,
            until: interest.shape.until,
            limit: interest.shape.limit,
            event_ids: interest.shape.event_ids.clone(),
            addresses: BTreeSet::new(),
        };

        // Case A: interest has explicit authors — partition them.
        // Also routes any address-pointer coordinates in the same interest, since
        // both authors and addresses may target the same relay (or different ones).
        if !interest.shape.authors.is_empty() {
            // relay_url → (author_set, addr_set, source)
            let mut per_relay: BTreeMap<RelayUrl, (BTreeSet<Pubkey>, BTreeSet<NaddrCoord>, RoutingSource)> =
                BTreeMap::new();

            for author in &interest.shape.authors {
                match self.mailbox_cache.get(author) {
                    Some(snapshot) => {
                        for relay in snapshot.outbox_relays() {
                            let entry =
                                per_relay.entry(relay.clone()).or_insert_with(|| {
                                    (BTreeSet::new(), BTreeSet::new(), RoutingSource::Nip65)
                                });
                            entry.0.insert(author.clone());
                        }
                    }
                    None => {
                        // Indexer fallback (Stage 2)
                        for relay in self.indexer_relays {
                            let entry =
                                per_relay.entry(relay.clone()).or_insert_with(|| {
                                    (BTreeSet::new(), BTreeSet::new(), RoutingSource::Indexer)
                                });
                            entry.0.insert(author.clone());
                        }
                    }
                }
            }

            // Route address-pointer coordinates (may target the same relays or different ones).
            for coord in &interest.shape.addresses {
                match self.mailbox_cache.get(&coord.pubkey) {
                    Some(snapshot) => {
                        for relay in snapshot.outbox_relays() {
                            let entry =
                                per_relay.entry(relay.clone()).or_insert_with(|| {
                                    (BTreeSet::new(), BTreeSet::new(), RoutingSource::Nip65)
                                });
                            entry.1.insert(coord.clone());
                        }
                    }
                    None => {
                        for relay in self.indexer_relays {
                            let entry =
                                per_relay.entry(relay.clone()).or_insert_with(|| {
                                    (BTreeSet::new(), BTreeSet::new(), RoutingSource::Indexer)
                                });
                            entry.1.insert(coord.clone());
                        }
                    }
                }
            }

            for (relay_url, (authors, addrs, source)) in per_relay {
                relay_entries.entry(relay_url).or_default().push(RelayEntry {
                    base_shape: base_shape.clone(),
                    authors_for_relay: authors,
                    addresses_for_relay: addrs,
                    lifecycle: interest.lifecycle.clone(),
                    source,
                    interest_id: interest.id.clone(),
                });
            }
            return;
        }

        // Case B: no explicit authors, but address-pointer pubkeys → Outbox.
        if !interest.shape.addresses.is_empty() {
            // relay_url → (coord_set, source)
            let mut per_relay_addrs: BTreeMap<RelayUrl, (BTreeSet<NaddrCoord>, RoutingSource)> =
                BTreeMap::new();

            for coord in &interest.shape.addresses {
                match self.mailbox_cache.get(&coord.pubkey) {
                    Some(snapshot) => {
                        for relay in snapshot.outbox_relays() {
                            let entry =
                                per_relay_addrs.entry(relay.clone()).or_insert_with(|| {
                                    (BTreeSet::new(), RoutingSource::Nip65)
                                });
                            entry.0.insert(coord.clone());
                        }
                    }
                    None => {
                        for relay in self.indexer_relays {
                            let entry =
                                per_relay_addrs.entry(relay.clone()).or_insert_with(|| {
                                    (BTreeSet::new(), RoutingSource::Indexer)
                                });
                            entry.0.insert(coord.clone());
                        }
                    }
                }
            }

            for (relay_url, (addrs, source)) in per_relay_addrs {
                relay_entries.entry(relay_url).or_default().push(RelayEntry {
                    base_shape: base_shape.clone(),
                    authors_for_relay: BTreeSet::new(),
                    addresses_for_relay: addrs,
                    lifecycle: interest.lifecycle.clone(),
                    source,
                    interest_id: interest.id.clone(),
                });
            }
            return;
        }

        // Case C: no author, no addresses — route to indexer set (e.g. hashtag firehose).
        for relay in self.indexer_relays {
            relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
                base_shape: base_shape.clone(),
                authors_for_relay: BTreeSet::new(),
                addresses_for_relay: BTreeSet::new(),
                lifecycle: interest.lifecycle.clone(),
                source: RoutingSource::Indexer,
                interest_id: interest.id.clone(),
            });
        }
    }
}

// ─── Plan-id hashing ─────────────────────────────────────────────────────────

/// Compute a stable, deterministic plan-id string.
///
/// The hash covers: sorted interest ids + shapes + scopes + lifecycles,
/// the mailbox snapshot sorted by pubkey, and the lattice version.
///
/// Phase 1: uses a simple FNV-1a accumulation. Phase 2 will upgrade to
/// blake3 when that crate is in the workspace.
fn compute_plan_id(
    interests: &[LogicalInterest],
    cache: &dyn MailboxCache,
    lattice_version: u8,
) -> String {
    struct FnvHasher(u64);
    impl FnvHasher {
        fn new() -> Self {
            Self(0xcbf29ce484222325)
        }
        fn feed_bytes(&mut self, bytes: &[u8]) {
            for &b in bytes {
                self.0 ^= u64::from(b);
                self.0 = self.0.wrapping_mul(0x100000001b3);
            }
        }
        fn finish(self) -> u64 {
            self.0
        }
    }

    let mut h = FnvHasher::new();

    // Sorted interest contributions
    let mut sorted_interests: Vec<&LogicalInterest> = interests.iter().collect();
    sorted_interests.sort_by_key(|i| &i.id);
    for interest in sorted_interests {
        // Feed id
        h.feed_bytes(&interest.id.0.to_le_bytes());
        // Feed shape via serde_json (deterministic because BTreeSet/BTreeMap)
        if let Ok(shape_json) = serde_json::to_vec(&interest.shape) {
            h.feed_bytes(&shape_json);
        }
        // Feed lifecycle tag byte
        let lifecycle_tag: u8 = match &interest.lifecycle {
            InterestLifecycle::Tailing => 0,
            InterestLifecycle::OneShot => 1,
            InterestLifecycle::BoundedTime { until_ms } => {
                h.feed_bytes(&until_ms.to_le_bytes());
                2
            }
        };
        h.feed_bytes(&[lifecycle_tag]);
    }

    // Sorted mailbox snapshot
    let mut snapshot = cache.snapshot_all();
    snapshot.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (pk, mb) in snapshot {
        h.feed_bytes(pk.as_bytes());
        for r in &mb.write_relays {
            h.feed_bytes(r.as_bytes());
        }
        for r in &mb.read_relays {
            h.feed_bytes(r.as_bytes());
        }
        for r in &mb.both_relays {
            h.feed_bytes(r.as_bytes());
        }
    }

    // Lattice version
    h.feed_bytes(&[lattice_version]);

    format!("{:016x}", h.finish())
}

// ─── Canonical filter hash ────────────────────────────────────────────────────

fn simple_shape_hash(shape: &InterestShape) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut h = DefaultHasher::new();
    if let Ok(json) = serde_json::to_string(shape) {
        json.hash(&mut h);
    }
    format!("{:08x}", h.finish() & 0xffff_ffff)
}
