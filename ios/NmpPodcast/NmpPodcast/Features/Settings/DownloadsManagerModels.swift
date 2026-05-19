import SwiftUI

// MARK: - View models

struct DownloadManagerRowData: Identifiable {
    let episode: Episode
    let showTitle: String
    let showAccent: Color
    let artworkURL: URL?
    let status: DownloadManagerStatus

    var id: UUID { episode.id }
}

enum DownloadManagerStatus {
    case queued
    case downloading(progress: Double, bytesWritten: Int64?, expectedBytes: Int64?)
    case failed(message: String)
    case downloaded(byteCount: Int64)

    var isActive: Bool {
        switch self {
        case .queued, .downloading: true
        case .failed, .downloaded: false
        }
    }

    var isQueued: Bool {
        if case .queued = self { return true }
        return false
    }

    var isFailed: Bool {
        if case .failed = self { return true }
        return false
    }

    var isDownloaded: Bool {
        if case .downloaded = self { return true }
        return false
    }

    var sortRank: Int {
        switch self {
        case .downloading: 0
        case .queued: 1
        case .failed: 2
        case .downloaded: 3
        }
    }

    var progressValue: Double? {
        if case .downloading(let progress, _, _) = self { return progress }
        return nil
    }

    var symbol: String {
        switch self {
        case .queued: "clock"
        case .downloading: "arrow.down.circle"
        case .failed: "exclamationmark.triangle.fill"
        case .downloaded: "checkmark.circle.fill"
        }
    }

    var tint: Color {
        switch self {
        case .queued: .orange
        case .downloading: .blue
        case .failed: AppTheme.Tint.error
        case .downloaded: .green
        }
    }

    var primaryLabel: String {
        switch self {
        case .queued: "Queued"
        case .downloading: "Downloading"
        case .failed: "Failed"
        case .downloaded: "Downloaded"
        }
    }

    var detailLabel: String {
        switch self {
        case .queued:
            return "Waiting to start"
        case .downloading(let progress, let bytesWritten, let expectedBytes):
            let pct = Int((progress * 100).rounded())
            if let expectedBytes, expectedBytes > 0 {
                let current = bytesWritten ?? Int64(Double(expectedBytes) * progress)
                return "\(pct)% - \(Self.formatSize(current)) of \(Self.formatSize(expectedBytes))"
            }
            if let bytesWritten {
                return "\(pct)% - \(Self.formatSize(bytesWritten))"
            }
            return "\(pct)%"
        case .failed(let message):
            return message.isEmpty ? "Tap actions to retry" : message
        case .downloaded(let byteCount):
            return Self.formatSize(byteCount)
        }
    }

    private static func formatSize(_ bytes: Int64) -> String {
        bytes.formattedFileSize
    }
}

enum DownloadManagerAction {
    case start
    case retry
    case cancel
    case clearFailed
    case delete
}

extension Double {
    var clampedDownloadProgress: Double {
        Swift.max(0, Swift.min(1, self))
    }
}
