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
