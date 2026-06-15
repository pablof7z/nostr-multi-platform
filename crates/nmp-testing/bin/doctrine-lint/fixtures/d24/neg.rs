// D24 negative fixture — none of these lines may fire. They exercise the
// substring-false-positive class (a different observer method, a longer
// identifier ending in the token) and a comment.

fn accepted_shapes(&self) {
    // Accepted: a DIFFERENT observer method — the `unregister_raw_event_observer`
    // substring class prior reviews flagged. Must never match this needle.
    self.unregister_raw_event_observer(id);
    // Accepted: a longer identifier ending in the banned token (left-boundary).
    self.force_notify_event_observers(event);
    // Accepted (comment): // notify_event_observers(&ev) fires once, in the seam.
}
