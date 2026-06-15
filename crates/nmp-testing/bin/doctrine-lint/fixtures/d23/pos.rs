// D23 positive fixture — store-insert sites OUTSIDE the accepted-event
// chokepoint that MUST fire (event-flow PR1 lock). Each is a new ingest ladder
// writing to the `EventStore` directly, bypassing `verify_and_persist`.

fn rogue_ingest_ladder(&mut self) {
    // (1) contiguous field access on `self` — the canonical store-insert shape.
    self.store.insert(verified, &provenance, received_at_ms);
    // (2) contiguous field access on a bound kernel handle.
    kernel.store.insert(v, &relay_url, 0);
    // (3) contiguous bareword binding named `store`.
    store.insert(ev, &url, ms);
    // (4) rustfmt-SPLIT method chain — the chokepoint's exact shape. A
    // single-line-only matcher would EVADE this; D23 catches it across lines.
    match self
        .store
        .insert(verified, &provenance, self.ingest_received_at_ms())
    {
        Ok(_) => {}
        Err(_) => {}
    }
    // (5) TRAILING COMMENT on the `.store` line then `.insert(` — the comment
    // must be stripped before the suffix check or this would evade.
    let _ = self.store // grab the event store
        .insert(verified, &provenance, ts);
}
