import Foundation

// MARK: - Progress fraction clamping

extension Double {
    /// Clamp a progress fraction into `0...1` for safe display. Several
    /// download / playback UI surfaces consume fractions that can briefly
    /// fall outside `0...1` (network ETag races, off-by-a-tick reports);
    /// every UI site that displays percentages needs to clamp before
    /// rendering, so the helper lives once here.
    var clamped01: Double { Swift.max(0, Swift.min(1, self)) }
}

// MARK: - Byte-count formatting

/// Shared, locale-aware `ByteCountFormatter` for download / storage UI.
/// `.file` style with `.useAll` lets iOS pick KB / MB / GB per locale.
/// Reentrant for `string(fromByteCount:)` after construction, so one
/// instance covers every "show me how big this is" surface — previously
/// each site (`SettingsView`, `StorageSettingsView`,
/// `DownloadsManagerModels`) minted its own per call.
nonisolated(unsafe) private let sharedByteCountFormatter: ByteCountFormatter = {
    let f = ByteCountFormatter()
    f.countStyle = .file
    f.allowedUnits = [.useAll]
    return f
}()

extension Int64 {
    /// Locale-aware file-size string (e.g. "1.4 MB", "230 KB", "12 bytes").
    /// Wraps a process-shared `ByteCountFormatter` so call sites don't
    /// allocate one each render.
    var formattedFileSize: String {
        sharedByteCountFormatter.string(fromByteCount: self)
    }
}
