// D23 negative fixture — none of these lines may fire. They exercise the
// accepted shapes: longer identifiers ending in `store` (substring class),
// plain map inserts, the LogicalInterest one-door pattern, and a comment.

fn accepted_shapes(&mut self) {
    // Accepted: `keystore` / `restore` / `event_store` are longer identifiers
    // ending in `store` — NOT the kernel event store. The left-boundary rule
    // (no preceding identifier char) excludes them.
    keystore.insert(key, value);
    restore.insert(snapshot);
    event_store.insert(e);
    // Accepted: a plain collection insert is not the event store.
    map.insert(key, value);
    // Accepted: the one-door pattern — register a LogicalInterest and let the
    // chokepoint (`verify_and_persist`) own persistence. No direct store write.
    self.register_logical_interest(interest);
    // Accepted (comment): // self.store.insert(...) is the chokepoint, here.
}
