// D24 positive fixture — observer-notify sites OUTSIDE the post-store fan-out
// seam that MUST fire (event-flow lock). Each is a scattered re-notify that
// would fire the host twice for one accepted event.

fn rogue_fanout(&self) {
    // (1) field-method call on `self` — the canonical fan-out shape.
    self.notify_event_observers(&kernel_event);
    // (2) bareword call.
    notify_event_observers(event);
    // (3) rustfmt-SPLIT chained call — receiver on the line above; the method
    // token + `(` stays atomic so the split form is caught too.
    self.kernel()
        .notify_event_observers(&kernel_event);
    // (4) METHOD/PAREN SPLIT — the method NAME on one line, the `(` on the next.
    self.kernel()
        .notify_event_observers
        (&kernel_event);
}
