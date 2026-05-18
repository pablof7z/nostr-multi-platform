import SwiftUI

// MARK: - Row content

struct DownloadsManagerRow: View {
    let row: DownloadManagerRowData
    let onAction: (DownloadManagerAction, DownloadManagerRowData) -> Void

    var body: some View {
        HStack(alignment: .top, spacing: AppTheme.Spacing.sm) {
            artwork

            VStack(alignment: .leading, spacing: AppTheme.Spacing.xs) {
                Text(row.episode.title)
                    .font(AppTheme.Typography.body)
                    .lineLimit(2)
                Text(row.showTitle)
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                statusLine
                if let progress = row.status.progressValue {
                    ProgressView(value: progress)
                        .progressViewStyle(.linear)
                        .tint(row.showAccent)
                        .accessibilityLabel("Download progress")
                        .accessibilityValue("\(Int((progress * 100).rounded())) percent")
                }
            }

            Spacer(minLength: AppTheme.Spacing.xs)
            actionsMenu
        }
        .padding(.vertical, AppTheme.Spacing.xs)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityLabel)
    }

    private var artwork: some View {
        Group {
            if let url = row.artworkURL {
                CachedAsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFill()
                    default:
                        artworkPlaceholder
                    }
                }
            } else {
                artworkPlaceholder
            }
        }
        .frame(width: 44, height: 44)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
    }

    private var artworkPlaceholder: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(row.showAccent.opacity(0.18))
            Image(systemName: "waveform")
                .font(.body.weight(.semibold))
                .foregroundStyle(row.showAccent)
        }
    }

    private var statusLine: some View {
        HStack(spacing: 6) {
            Label(row.status.primaryLabel, systemImage: row.status.symbol)
                .labelStyle(.titleAndIcon)
                .font(AppTheme.Typography.caption2)
                .foregroundStyle(row.status.tint)
            Text(row.status.detailLabel)
                .font(AppTheme.Typography.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }

    private var actionsMenu: some View {
        Menu {
            switch row.status {
            case .queued:
                Button {
                    onAction(.start, row)
                } label: {
                    Label("Start Download", systemImage: "arrow.down.circle")
                }
                Button(role: .destructive) {
                    onAction(.cancel, row)
                } label: {
                    Label("Remove from Queue", systemImage: "xmark.circle")
                }
            case .downloading:
                Button(role: .destructive) {
                    onAction(.cancel, row)
                } label: {
                    Label("Cancel Download", systemImage: "xmark.circle")
                }
            case .failed:
                Button {
                    onAction(.retry, row)
                } label: {
                    Label("Retry Download", systemImage: "arrow.clockwise")
                }
                Button {
                    onAction(.clearFailed, row)
                } label: {
                    Label("Clear Failed State", systemImage: "xmark.circle")
                }
            case .downloaded:
                Button(role: .destructive) {
                    onAction(.delete, row)
                } label: {
                    Label("Delete Download", systemImage: "trash")
                }
            }
        } label: {
            Image(systemName: "ellipsis.circle")
                .font(.title3)
                .foregroundStyle(.secondary)
                .frame(width: 36, height: 36)
        }
        .accessibilityLabel("Download actions")
    }

    private var accessibilityLabel: String {
        "\(row.episode.title), \(row.showTitle), \(row.status.primaryLabel), \(row.status.detailLabel)"
    }
}

// MARK: - Summary stat

struct DownloadsSummaryStat: View {
    let value: Int
    let label: String
    let tint: Color

    var body: some View {
        VStack(spacing: 4) {
            Text("\(value)")
                .font(AppTheme.Typography.title)
                .monospacedDigit()
                .foregroundStyle(tint)
            Text(label)
                .font(AppTheme.Typography.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(value) \(label)")
    }
}
