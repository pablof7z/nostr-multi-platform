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
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.map.get_mut(key)
    }

    /// Whether `key` is present.
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
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.map.iter()
    }

    /// Iterate values in insertion order (oldest first).
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.map.values()
    }

    /// Iterate values mutably in insertion order.
    ///
    /// Mutation through this handle does **not** affect eviction order —
    /// only [`Self::insert`] can move entries to the back.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_map_is_empty_and_zero_length() {
        let map: BoundedMessageMap<String, u32> = BoundedMessageMap::new(8);
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 8);
    }

    #[test]
    fn insert_below_capacity_grows_normally() {
        let mut map = BoundedMessageMap::new(3);
        assert!(map.insert("a".to_string(), 1).is_none());
        assert!(map.insert("b".to_string(), 2).is_none());
        assert!(map.insert("c".to_string(), 3).is_none());
        assert_eq!(map.len(), 3);
        assert_eq!(map.get(&"a".to_string()), Some(&1));
        assert_eq!(map.get(&"b".to_string()), Some(&2));
        assert_eq!(map.get(&"c".to_string()), Some(&3));
    }

    #[test]
    fn insert_at_capacity_evicts_oldest() {
        let mut map = BoundedMessageMap::new(3);
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        map.insert("c".to_string(), 3);

        // Inserting a fourth distinct key must evict "a" (the oldest).
        map.insert("d".to_string(), 4);

        assert_eq!(map.len(), 3, "length must stay at capacity after eviction");
        assert!(
            map.get(&"a".to_string()).is_none(),
            "oldest entry must be evicted"
        );
        assert_eq!(map.get(&"b".to_string()), Some(&2));
        assert_eq!(map.get(&"c".to_string()), Some(&3));
        assert_eq!(map.get(&"d".to_string()), Some(&4));
    }

    #[test]
    fn many_inserts_keep_len_capped() {
        let mut map = BoundedMessageMap::new(5);
        for i in 0..100u32 {
            map.insert(format!("k{i}"), i);
        }
        assert_eq!(map.len(), 5);
        // The 5 newest keys are present; everything else has been evicted.
        for i in 95..100 {
            assert_eq!(map.get(&format!("k{i}")), Some(&i));
        }
        for i in 0..95 {
            assert!(
                map.get(&format!("k{i}")).is_none(),
                "k{i} must have been evicted",
            );
        }
    }

    #[test]
    fn re_inserting_existing_key_updates_in_place_without_eviction() {
        let mut map = BoundedMessageMap::new(3);
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        map.insert("c".to_string(), 3);

        // Re-insert "a" — the entry stays at the front position, "a" is NOT
        // evicted, and the previous value is returned.
        let prior = map.insert("a".to_string(), 11);
        assert_eq!(prior, Some(1));
        assert_eq!(map.len(), 3, "re-insert must not change length");

        // Now insert a new "d" — the front entry is still "a", so "a" gets
        // evicted (insertion-order eviction, not last-touch).
        map.insert("d".to_string(), 4);
        assert!(
            map.get(&"a".to_string()).is_none(),
            "re-inserting an existing key must NOT shift it to the back; it remains the oldest",
        );
        assert_eq!(map.get(&"b".to_string()), Some(&2));
        assert_eq!(map.get(&"c".to_string()), Some(&3));
        assert_eq!(map.get(&"d".to_string()), Some(&4));
    }

    #[test]
    fn iter_returns_entries_in_insertion_order() {
        let mut map = BoundedMessageMap::new(4);
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        map.insert("c".to_string(), 3);

        let keys: Vec<&String> = map.iter().map(|(k, _)| k).collect();
        let key_strs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
        assert_eq!(key_strs, vec!["a", "b", "c"]);

        let values: Vec<&u32> = map.values().collect();
        assert_eq!(values, vec![&1, &2, &3]);
    }

    #[test]
    fn iteration_after_eviction_skips_evicted_entries() {
        let mut map = BoundedMessageMap::new(2);
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        map.insert("c".to_string(), 3); // evicts "a"

        let keys: Vec<String> = map.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn get_mut_updates_value_without_changing_position() {
        let mut map = BoundedMessageMap::new(3);
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        map.insert("c".to_string(), 3);

        // Mutate "a" in place. The order must not change.
        if let Some(v) = map.get_mut(&"a".to_string()) {
            *v = 11;
        }
        assert_eq!(map.get(&"a".to_string()), Some(&11));

        // "a" is still the oldest — a new "d" evicts it.
        map.insert("d".to_string(), 4);
        assert!(map.get(&"a".to_string()).is_none());
    }

    #[test]
    fn remove_takes_an_entry_without_disturbing_others() {
        let mut map = BoundedMessageMap::new(3);
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        map.insert("c".to_string(), 3);

        assert_eq!(map.remove(&"b".to_string()), Some(2));
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&"b".to_string()), None);

        // The surviving entries keep their relative order.
        let keys: Vec<String> = map.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn remove_of_absent_key_is_none() {
        let mut map: BoundedMessageMap<String, u32> = BoundedMessageMap::new(2);
        map.insert("a".to_string(), 1);
        assert_eq!(map.remove(&"missing".to_string()), None);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn contains_key_reflects_insertions_and_evictions() {
        let mut map = BoundedMessageMap::new(2);
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        assert!(map.contains_key(&"a".to_string()));
        assert!(map.contains_key(&"b".to_string()));

        map.insert("c".to_string(), 3); // evicts "a"
        assert!(!map.contains_key(&"a".to_string()));
        assert!(map.contains_key(&"b".to_string()));
        assert!(map.contains_key(&"c".to_string()));
    }

    #[test]
    fn capacity_zero_degrades_to_one_not_panic() {
        // A pathological `new(0)` would otherwise evict every entry on
        // insertion. The min-of-one guard keeps the type safe to construct
        // from arbitrary configuration.
        let mut map = BoundedMessageMap::new(0);
        assert_eq!(map.capacity(), 1);
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        // Only the newest survives.
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&"b".to_string()), Some(&2));
        assert!(map.get(&"a".to_string()).is_none());
    }

    #[test]
    fn is_empty_tracks_state() {
        let mut map = BoundedMessageMap::new(2);
        assert!(map.is_empty());
        map.insert("a".to_string(), 1);
        assert!(!map.is_empty());
        map.remove(&"a".to_string());
        assert!(map.is_empty());
    }

    #[test]
    fn production_capacity_constant_is_ten_thousand() {
        // Pin the constant so any change is a deliberate one — every
        // projection initialises with this value, so it is part of the wire
        // contract of "how big can a projection get in steady state".
        assert_eq!(MAX_PROJECTION_MESSAGES, 10_000);
    }

    #[test]
    fn entry_or_insert_with_inserts_when_absent() {
        let mut map: BoundedMessageMap<String, u32> = BoundedMessageMap::new(4);
        let v = map.entry_or_insert_with("a".to_string(), || 42);
        assert_eq!(*v, 42);
        // Mutate through the returned reference to confirm it really is &mut V.
        *v = 43;
        assert_eq!(map.get(&"a".to_string()), Some(&43));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn entry_or_insert_with_returns_existing_when_present() {
        let mut map = BoundedMessageMap::new(3);
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        map.insert("c".to_string(), 3);

        // Touching an existing key must NOT call the default and must NOT
        // change length or eviction order.
        let v = map.entry_or_insert_with("a".to_string(), || {
            panic!("default closure must not run for an existing key");
        });
        assert_eq!(*v, 1);
        assert_eq!(map.len(), 3);

        // "a" is still the oldest — inserting "d" still evicts it, proving the
        // entry call did not shift it to the back.
        map.insert("d".to_string(), 4);
        assert!(map.get(&"a".to_string()).is_none());
        assert_eq!(map.get(&"b".to_string()), Some(&2));
        assert_eq!(map.get(&"c".to_string()), Some(&3));
        assert_eq!(map.get(&"d".to_string()), Some(&4));
    }

    #[test]
    fn entry_or_insert_with_evicts_oldest_when_full() {
        let mut map = BoundedMessageMap::new(3);
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        map.insert("c".to_string(), 3);

        // At capacity — entry on a NEW key must evict the oldest ("a").
        let v = map.entry_or_insert_with("d".to_string(), || 4);
        assert_eq!(*v, 4);
        assert_eq!(map.len(), 3, "length must stay at capacity after eviction");
        assert!(
            map.get(&"a".to_string()).is_none(),
            "oldest entry must be evicted",
        );
        assert_eq!(map.get(&"b".to_string()), Some(&2));
        assert_eq!(map.get(&"c".to_string()), Some(&3));
        assert_eq!(map.get(&"d".to_string()), Some(&4));
    }

    #[test]
    fn first_returns_oldest_entry_or_none_when_empty() {
        let mut map: BoundedMessageMap<String, u32> = BoundedMessageMap::new(2);
        assert!(map.first().is_none());
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        assert_eq!(map.first(), Some((&"a".to_string(), &1)));
        map.insert("c".to_string(), 3); // at capacity — "a" evicted, "b" is now oldest
        assert_eq!(map.first(), Some((&"b".to_string(), &2)));
    }

    #[test]
    fn insert_returning_evicted_reports_evicted_pair() {
        let mut map = BoundedMessageMap::new(2);
        map.insert("a".to_string(), 10u32);
        map.insert("b".to_string(), 20u32);

        // At capacity — inserting a new key evicts "a".
        let (prev, evicted) = map.insert_returning_evicted("c".to_string(), 30u32);
        assert!(prev.is_none(), "new key has no prior value");
        assert_eq!(evicted, Some(("a".to_string(), 10u32)));
        assert_eq!(map.len(), 2);
        assert!(map.get(&"a".to_string()).is_none());
        assert_eq!(map.get(&"b".to_string()), Some(&20));
        assert_eq!(map.get(&"c".to_string()), Some(&30));
    }

    #[test]
    fn insert_returning_evicted_update_in_place_yields_no_eviction() {
        let mut map = BoundedMessageMap::new(2);
        map.insert("a".to_string(), 1u32);
        map.insert("b".to_string(), 2u32);

        // Re-inserting an existing key updates in place — no eviction.
        let (prev, evicted) = map.insert_returning_evicted("a".to_string(), 99u32);
        assert_eq!(prev, Some(1u32));
        assert!(evicted.is_none());
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&"a".to_string()), Some(&99));
    }

    #[test]
    fn insert_returning_evicted_below_capacity_yields_no_eviction() {
        let mut map = BoundedMessageMap::new(3);
        map.insert("a".to_string(), 1u32);

        let (prev, evicted) = map.insert_returning_evicted("b".to_string(), 2u32);
        assert!(prev.is_none());
        assert!(evicted.is_none(), "below capacity, no eviction occurs");
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn entry_or_default_convenience() {
        let mut map: BoundedMessageMap<String, u32> = BoundedMessageMap::new(2);
        // Absent key — default value is inserted.
        let v = map.entry_or_default("a".to_string());
        assert_eq!(*v, 0);
        *v = 7;
        assert_eq!(map.get(&"a".to_string()), Some(&7));

        // Present key — existing value is returned, no overwrite.
        let v = map.entry_or_default("a".to_string());
        assert_eq!(*v, 7);
        assert_eq!(map.len(), 1);
    }
}
