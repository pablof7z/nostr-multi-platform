import Foundation
import os.log

private let kmLog = Logger(subsystem: "io.f7z.podcast", category: "KernelModel")

/// The top-level SwiftUI model for the Podcast shell. Holds one `KernelHandle`
/// (the Rust actor), publishes the decoded `PodcastUpdate` snapshot, and
/// surfaces lifecycle / error state to the UI. No business logic lives here
/// (D7): the kernel decides what every event means.
@MainActor
final class KernelModel: ObservableObject {
    // ── Published state ───────────────────────────────────────────────────

    @Published private(set) var snapshot: PodcastUpdate?

    /// `true` when the Rust actor has panicked or shut down. The UI shows a
    /// non-dismissible fatal-error banner and a "Relaunch" button (exit(0)).
    @Published private(set) var kernelIsDead: Bool = false

    /// Short human-readable error text. Cleared after 4 s or on user tap.
    @Published private(set) var lastErrorToast: String?

    /// Synchronous dispatch-rejection message (shown as a secondary toast when
    /// the kernel rejects an action before it can be enqueued).
    @Published private(set) var lastDispatchError: String?

    // ── Internal ──────────────────────────────────────────────────────────

    private let kernel = KernelHandle()
    private var isRunning = false

    // ── Lifecycle ─────────────────────────────────────────────────────────

    func start() {
        guard !isRunning else { return }
        isRunning = true
        kernel.listen({ [weak self] result in
            Task { @MainActor [weak self] in
                self?.apply(result.update)
            }
        }, onPanic: { [weak self] in
            Task { @MainActor [weak self] in
                self?.kernelIsDead = true
            }
        })
        kernel.start()
    }

    func stop() {
        kernel.stop()
        isRunning = false
    }

    /// D7 pull-side actor-liveness probe (ADR-0028). Call on foreground resume
    /// to catch panics that happened while the app was backgrounded.
    func checkAlive() {
        if !kernel.isAlive() {
            kernelIsDead = true
        }
    }

    func lifecycleForeground() {
        kernel.lifecycleForeground()
    }

    func lifecycleBackground() {
        kernel.lifecycleBackground()
    }

    // ── Snapshot application ──────────────────────────────────────────────

    private func apply(_ update: PodcastUpdate) {
        snapshot = update
        if let toast = update.lastErrorToast, !toast.isEmpty {
            lastErrorToast = toast
        }
    }

    // ── Convenience accessors ─────────────────────────────────────────────

    var isKernelRunning: Bool { snapshot?.running == true }

    // ── Toast management ──────────────────────────────────────────────────

    func clearErrorToast() {
        lastErrorToast = nil
    }
}
