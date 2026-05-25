import Foundation
import os.log

// ── KernelUpdateSink ─────────────────────────────────────────────────────

/// Holds the Swift-side callback closures wired to the kernel's update channel.
/// Passed to `nmpUpdateCallback` as the opaque `context` pointer. Kept alive
/// by `KernelHandle.updateSink` so it is never freed while the callback may fire.
final class KernelUpdateSink {
    let handler: (KernelUpdateResult) -> Void
    /// D7 actor-death hook. Runs exactly once when the Rust supervisor emits
    /// `{"t":"panic",...}` on the update channel. The host uses this to flip a
    /// `@Published` flag and show a fatal-error banner.
    let onPanic: () -> Void

    init(
        handler: @escaping (KernelUpdateResult) -> Void,
        onPanic: @escaping () -> Void
    ) {
        self.handler = handler
        self.onPanic = onPanic
    }
}

// ── C update callback ────────────────────────────────────────────────────

/// C update callback — fires on every kernel snapshot frame and on actor death.
/// Context is `KernelUpdateSink`, passed unretained from `KernelHandle.listen`.
let nmpUpdateCallback: NmpUpdateCallback = { context, pointer in
    guard let context, let pointer else { return }
    let payload = String(cString: pointer)
    let sink = Unmanaged<KernelUpdateSink>.fromOpaque(context).takeUnretainedValue()
    // D7 actor-death contract: the Rust supervisor emits exactly one
    // `{"t":"panic","v":{"msg":...}}` envelope before the channel closes.
    if payload.contains("\"t\":\"panic\"") {
        kbLog.fault("NMP_ACTOR_PANIC detected bytes=\(payload.utf8.count)")
        sink.onPanic()
        return
    }
    guard let result = KernelHandle.decode(pointer: pointer) else { return }
    sink.handler(result)
}
