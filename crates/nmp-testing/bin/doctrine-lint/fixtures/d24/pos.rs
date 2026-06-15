// D24 positive fixture — observer-notify sites OUTSIDE the post-store fan-out
// seam that MUST fire (event-flow lock). Each is a scattered re-notify that
// would fire the host twice for one accepted event.

fn rogue_fanout(&self) {
    // (1) field-method call on `self` — the canonical fan-out shape.
    self.notify_event_observers(&kernel_event);
    // (2) bareword call.
    notify_event_observers(event);
}
