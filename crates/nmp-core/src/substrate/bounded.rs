//! `BoundedMessageMap<K, V>` — a hash map with a hard capacity that evicts the
//! oldest entry by insertion order when full.
//!
//! # Why a primitive
//!
//! Three live snapshot projections retain unbounded per-event state that is
//! re-serialised on every snapshot tick (≈4 Hz):
//!
//! * `nmp_nip29::projection::group_chat::GroupChatProjection`
//!   — chat messages keyed by event id.
//! * `nmp_nip17::inbox::DmInboxProjection`
//!   — decrypted DM rumors keyed by inner-rumor event id.
//! * `nmp_nip57::projection::ZapsAggregateProjection`
//!   — per-target receipt sets keyed by target event id.
//!
//! Each one had its own ad-hoc `BTreeMap` / `HashMap` that grew linearly with
//! session length. With ~10 000 messages at 250 bytes each, re-serialising at
//! 4 Hz produces ~10 MB/s of redundant snapshot work and the resident set
//! never shrinks. This primitive replaces the unbounded map with an
//! `IndexMap`-backed store that:
//!
//! 1. preserves insertion order, so "oldest entry" is well-defined,
//! 2. evicts the front entry when `insert` would exceed `capacity`, and
//! 3. updates in place when re-inserting an existing key (no eviction, no
//!    position shift) — so idempotent re-delivery of the same event id keeps
//!    behaving the way the BTreeMap-backed code does today.
//!
//! Recency-over-completeness is the right trade-off for projection stores:
//! the snapshot is a *render-ready* view, not a durable log. The underlying
//! event store retains the full history; the projection is free to forget
//! the oldest rows once it has saturated its working set.
//!
//! # Capacity choice
//!
//! [`MAX_PROJECTION_MESSAGES`] is the cap every projection initialises with.
//! It sits well above any single screen's working set (a chat thread or a
//! DM inbox) but low enough that the bounded snapshot stays cheap to
//! serialise on every tick. Tune the constant — never thread `capacity`
//! through every call site — so the bound stays one number.
//!
//! # Doctrine
//!
//! * **D0 / D8** — no app nouns; no I/O; cheap (single map operation per
//!   call). Safe to invoke on the actor thread.
//! * **D6** — `BoundedMessageMap` itself never panics; callers that hold it
//!   behind a `Mutex` keep their existing poisoned-mutex degrade-to-empty
//!   behaviour.

use std::hash::Hash;

use indexmap::IndexMap;

/// Hard cap every projection's message store is initialised with.
///
/// Tuned for the projection workload: a chat thread or DM inbox rarely needs
/// more than a few thousand rows on screen, and the snapshot tick at ~4 Hz
/// must finish before the next one starts. 10 000 leaves headroom for
/// busy NIP-29 group channels while keeping the snapshot serialisation
/// budget bounded.
pub const MAX_PROJECTION_MESSAGES: usize = 10_000;

/// A bounded hash map that evicts the oldest entry (by insertion order) when
/// inserting into a full map.
///
/// Built on [`indexmap::IndexMap`] for O(1) hash lookup plus O(1) access to
/// the oldest-by-insertion-order entry; the actual eviction is one
/// `shift_remove_index(0)` which is O(n) in the bounded `capacity`. With the
/// production cap of [`MAX_PROJECTION_MESSAGES`], the eviction cost is
/// constant in steady state.
///
/// Re-inserting an existing key updates the value in place and **does not**
/// shift the entry to the back — eviction order is *insertion* order, not
/// *last-touch* order. This matches the idempotency contract the existing
/// projections rely on: a re-delivered event id replaces rather than
/// duplicates, and never delays its own eventual eviction.
#[derive(Debug, Clone)]
pub struct BoundedMessageMap<K, V> {
    map: IndexMap<K, V>,
    capacity: usize,
}

impl<K, V> BoundedMessageMap<K, V>
where
    K: Eq + Hash,
{
    /// Construct an empty map bounded by `capacity`. A `capacity` of `0`
    /// silently behaves as `1` — a degenerate value that would otherwise make
    /// every `insert` immediately evict itself. The minimum-of-one guard
    /// keeps the type safe to construct from configuration without a panic.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            map: IndexMap::with_capacity(capacity),
            capacity,
        }
    }

    /// Insert `(key, value)`.
    ///
    /// * If `key` is already present, the entry's position is preserved and
    ///   the previous value is returned (mirrors `HashMap::insert` semantics).
    /// * If `key` is new and the map is at capacity, the oldest entry (front
    ///   of the insertion order) is evicted *before* the new entry is added,
    ///   so `len()` never exceeds `capacity`. The displaced value of the
    ///   *evicted* entry is discarded; the return value is still the prior
    ///   value of `key` itself, which is `None` in this branch.
    ///
    /// This is the only mutation method that can shrink the map by eviction;
    /// callers that need explicit removal should reach for [`Self::remove`].
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if self.map.contains_key(&key) {
            // Update-in-place: preserves position, no eviction.
            return self.map.insert(key, value);
        }
        if self.map.len() >= self.capacity {
            // At capacity — evict the front-most (oldest) entry to make room.
            // `shift_remove_index(0)` preserves the relative order of the
            // surviving entries, which is what "evict oldest" requires.
            self.map.shift_remove_index(0);
        }
        self.map.insert(key, value)
    }

    /// Borrow the value for `key`, or `None` if absent.
    ///
    /// Accepts any `Q` where `K: Borrow<Q>`, so callers can pass `&str` when
    /// the map is keyed on `String` (mirrors `HashMap::get` ergonomics).
    #[must_use]
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.map.get(key)
    }

    /// Mutably borrow the value for `key`, or `None` if absent.
    ///
    /// Mutating an existing value through this handle does **not** affect
    /// eviction order — only [`Self::insert`] adds to the back. This is the
    /// hook the `ZapsAggregateProjection` migration uses to update the inner
    /// receipt map without touching the outer position.
    ///
    /// Accepts `Q` for the same ergonomic reason as [`Self::get`].
    #[must_use]
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.map.get_mut(key)
    }

    /// Whether `key` is present.
    #[must_use]
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.map.contains_key(key)
    }

    /// Remove `key`, returning its value if present. The remaining entries
    /// preserve their relative insertion order (this is `shift_remove`, not
    /// `swap_remove`).
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.map.shift_remove(key)
    }

    /// Number of entries currently in the map.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the map holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The capacity bound this map was constructed with. `len() <= capacity()`
    /// is an invariant.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Iterate `(key, value)` pairs in insertion order (oldest first).
    #[must_use]
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.map.iter()
    }

    /// Iterate values in insertion order (oldest first).
    #[must_use]
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.map.values()
    }

    /// Iterate values mutably in insertion order.
    ///
    /// Mutation through this handle does **not** affect eviction order —
    /// only [`Self::insert`] can move entries to the back.
    #[must_use]
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.map.values_mut()
    }

    /// Return a mutable reference to the value under `key`, inserting
    /// `default()` if the key is absent.
    ///
    /// Eviction contract: if `key` is not present and the map is at capacity,
    /// the oldest-by-insertion-order entry is evicted before the new one is
    /// inserted. Re-accessing an existing key updates the value in place
    /// (no eviction, no position change) — same contract as [`Self::insert`].
    pub fn entry_or_insert_with<F>(&mut self, key: K, default: F) -> &mut V
    where
        F: FnOnce() -> V,
    {
        use indexmap::map::Entry;
        // Only evict when we're about to insert a NEW key at capacity.
        if !self.map.contains_key(&key) && self.map.len() >= self.capacity {
            self.map.shift_remove_index(0);
        }
        match self.map.entry(key) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => e.insert(default()),
        }
    }

    /// Variant of [`Self::entry_or_insert_with`] for `V: Default`.
    pub fn entry_or_default(&mut self, key: K) -> &mut V
    where
        V: Default,
    {
        self.entry_or_insert_with(key, V::default)
    }

    /// Borrow the oldest-by-insertion-order `(key, value)` pair, or `None`
    /// when the map is empty. Useful when callers need to react to eviction:
    /// peek at the oldest entry *before* calling [`Self::insert`] at capacity.
    #[must_use]
    pub fn first(&self) -> Option<(&K, &V)> {
        self.map.first()
    }

    /// Insert `(key, value)`, returning both the displaced prior value for
    /// `key` and the evicted `(key, value)` pair if the map was at capacity
    /// and a new key was added. The second element is `None` when the key was
    /// already present (update-in-place, no eviction) or when the map was
    /// below capacity.
    #[must_use]
    pub fn insert_returning_evicted(&mut self, key: K, value: V) -> (Option<V>, Option<(K, V)>)
    where
        K: Clone,
    {
        if self.map.contains_key(&key) {
            return (self.map.insert(key, value), None);
        }
        let evicted = if self.map.len() >= self.capacity {
            if let Some((ek, _)) = self.map.get_index(0) {
                let ek = ek.clone();
                let ev = self.map.shift_remove(&ek);
                ev.map(|v| (ek, v))
            } else {
                None
            }
        } else {
            None
        };
        (self.map.insert(key, value), evicted)
    }
}

/// A bounded FIFO ring of values: `push` appends to the back and, when the
/// ring is at capacity, evicts the oldest (front) value first. Iteration
/// yields values in arrival order (oldest first).
///
/// Unlike [`BoundedMessageMap`] this is keyless and append-only — the right
/// shape for an ordered *signal stream* (e.g. "these event ids were released
/// without a match") that an observer drains in arrival order, rather than a
/// keyed projection store. Duplicates are allowed; the ring is a log, not a
/// set.
///
/// Doctrine: same as [`BoundedMessageMap`] — D0/D8 (no app nouns, no I/O,
/// O(1) push/evict), D5 (capacity-bounded; resident set never exceeds
/// `capacity`).
#[derive(Debug, Clone)]
pub struct BoundedRing<T> {
    ring: std::collections::VecDeque<T>,
    capacity: usize,
}

impl<T> BoundedRing<T> {
    /// Construct an empty ring bounded by `capacity`. A `capacity` of `0`
    /// silently behaves as `1` (mirrors [`BoundedMessageMap::new`]).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            ring: std::collections::VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Append `value` to the back. If the ring is at capacity, the oldest
    /// (front) value is evicted first so `len()` never exceeds `capacity`.
    pub fn push(&mut self, value: T) {
        if self.ring.len() >= self.capacity {
            self.ring.pop_front();
        }
        self.ring.push_back(value);
    }

    /// Number of values currently in the ring.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// Whether the ring holds no values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// The capacity bound this ring was constructed with.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Iterate values in arrival order (oldest first).
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.ring.iter()
    }
}

#[cfg(test)]
#[path = "bounded/tests.rs"]
mod tests;
