// D25 positive fixture — direct REQ-build sites OUTSIDE the subscription
// compiler / lifecycle that MUST fire (acquisition one-door, Workstream B4).
// Each builds a relay REQ directly, bypassing LogicalInterest accounting.

fn rogue_request_helper(&self) {
    // (1) method call on a kernel handle — the canonical direct-REQ shape.
    let _r = kernel.req_for_relay(role, relay_url, sub_id, summary, filter);
    // (2) bareword call.
    req_for_relay(role, url, id, summary, filter);
    // (3) rustfmt-SPLIT chained call — receiver on the line above; the method
    // token + `(` stays atomic so the split form is caught too.
    let _s = self
        .kernel()
        .req_for_relay(role, url, id, summary, filter);
}
