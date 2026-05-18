import SwiftUI

/// Scrollable list of transcript segments. Tapping a segment invokes
/// `onTapSegment` — typical wiring is: seek to `segment.start` and extend
/// the clip range to cover it. Auto-scrolls to the currently-playing row.
struct TranscriptView: View {
    let segments: [TranscriptSegment]
    let currentTime: TimeInterval
    let selectedSegmentIds: Set<String>
    let onTapSegment: (TranscriptSegment) -> Void

    var body: some View {
        ScrollViewReader { reader in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 4) {
                    ForEach(segments) { segment in
                        Button {
                            onTapSegment(segment)
                        } label: {
                            row(for: segment)
                        }
                        .buttonStyle(.plain)
                        .id(segment.id)
                    }
                }
                .padding(.vertical, 4)
            }
            .onChange(of: activeSegmentId) { _, newId in
                guard let newId else { return }
                withAnimation(.easeInOut(duration: 0.2)) {
                    reader.scrollTo(newId, anchor: .center)
                }
            }
        }
    }

    private var activeSegmentId: String? {
        segments.first { currentTime >= $0.start && currentTime < $0.end }?.id
    }

    @ViewBuilder
    private func row(for segment: TranscriptSegment) -> some View {
        let isActive = currentTime >= segment.start && currentTime < segment.end
        let isSelected = selectedSegmentIds.contains(segment.id)

        VStack(alignment: .leading, spacing: 2) {
            if !segment.speaker.isEmpty {
                Text(segment.speaker)
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
            }
            Text(segment.text)
                .font(.body)
                .foregroundStyle(isActive ? .primary : .secondary)
                .multilineTextAlignment(.leading)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(background(isActive: isActive, isSelected: isSelected))
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .contentShape(Rectangle())
    }

    @ViewBuilder
    private func background(isActive: Bool, isSelected: Bool) -> some View {
        if isSelected {
            Color.highlighterAccent.opacity(0.15)
        } else if isActive {
            Color.secondary.opacity(0.12)
        } else {
            Color.clear
        }
    }
}
