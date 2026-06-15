// D25 negative fixture — none of these lines may fire. They exercise the
// accepted one-door pattern, a longer identifier ending in the banned token,
// and a comment.

fn accepted_shapes(&self) {
    // Accepted: a longer identifier ending in the token (left-boundary rule).
    let _ = build_req_for_relay(role);
    // Accepted: the one-door pattern — register a LogicalInterest and let the
    // planner-owned compiler emit the REQ. No direct REQ build here.
    self.register_logical_interest(interest);
    // Accepted (comment): // req_for_relay() lives only in the compiler.
}
