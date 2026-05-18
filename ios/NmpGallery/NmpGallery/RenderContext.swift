import Foundation

// PROJECTION-GAP NOTE (#2): `nmp_content::RenderContext` is non-serde with
// no FFI projection. The STAGE 2 bundle carries resolution facts only; the
// PD-015 depth budget + `visited`-set cycle guard is a render-time concern
// that travels with the renderer's traversal. This is the faithful Swift
// mirror of `RenderContext::should_collapse`:
//
//   depth >= max_depth (default 4)  OR  visited.contains(into)
//
// keyed exactly like the Rust substrate `EventId` (a String): event id
// hex for notes/nevents, `kind:pubkey:d` coordinate for addressable
// events.
struct RenderContext {
    var depth: Int = 0
    var maxDepth: Int = 4
    var visited: Set<String> = []

    func shouldCollapse(into key: String) -> (Bool, String?) {
        if visited.contains(key) { return (true, "cycle") }
        if depth >= maxDepth { return (true, "depth") }
        return (false, nil)
    }

    func descend(into key: String) -> RenderContext {
        var next = self
        next.depth += 1
        next.visited.insert(key)
        return next
    }
}

/// Stable visited/cycle key for a resolved embed event, mirroring how the
/// Rust renderer keys `RenderContext.visited`.
func visitedKey(for ev: SignedEventJson) -> String {
    let addressable = (30000..<40000).contains(ev.kind) || ev.kind == 10002
    if addressable {
        let d = ev.tags.first { $0.first == "d" }?
            .dropFirst().first ?? ""
        return "\(ev.kind):\(ev.pubkey):\(d)"
    }
    return ev.id
}
