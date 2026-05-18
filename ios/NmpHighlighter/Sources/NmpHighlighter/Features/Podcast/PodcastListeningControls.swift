import SwiftUI

extension PodcastListeningView {
    var availableChapters: [Chapter] {
        player.currentArtifact?.preview.chapters ?? []
    }

    var timelineRows: [TimelineRow] {
        var rows: [TimelineRow] = []

        if showClips {
            for h in memberClips {
                rows.append(.clip(h))
            }
        }

        if showChapters {
            for chapter in availableChapters {
                rows.append(.chapter(t: chapter.startSeconds, title: chapter.title))
            }
        }

        if showTranscript && player.transcriptAvailability == .available {
            for seg in player.transcriptSegments {
                rows.append(.transcript(seg))
            }
        } else {
            let occupiedTimes = rows.map { $0.t }
            let totalDuration = player.duration > 0 ? player.duration : 3600
            var t: Double = 0
            while t < totalDuration {
                let hasNeighbor = occupiedTimes.contains { abs($0 - t) < 8 }
                if !hasNeighbor {
                    rows.append(.waveformTick(t: t))
                }
                t += waveformTickWindow
            }
        }

        return rows.sorted { $0.t < $1.t }
    }

    // MARK: - Audio pill

    var audioPill: some View {
        HStack(spacing: 14) {
            Button {
                player.toggle()
            } label: {
                ZStack {
                    Circle()
                        .fill(Color.primary)
                        .frame(width: 40, height: 40)
                    if player.isBuffering {
                        ProgressView()
                            .controlSize(.small)
                            .tint(Color(.systemBackground))
                    } else {
                        Image(systemName: player.isPlaying ? "pause.fill" : "play.fill")
                            .font(.system(size: 16, weight: .semibold))
                            .foregroundStyle(Color(.systemBackground))
                    }
                }
            }
            .buttonStyle(.plain)

            VStack(alignment: .leading, spacing: 2) {
                Text("now playing")
                    .font(.caption2)
                    .foregroundStyle(.secondary)

                Text(currentSpeakerOrTimestamp)
                    .font(.caption.weight(.semibold).monospacedDigit())
                    .foregroundStyle(.primary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)

            // Progress strip
            GeometryReader { geo in
                let fraction: Double = player.duration > 0
                    ? min(1, max(0, player.currentTime / player.duration))
                    : 0
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Color(.separator))
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Color.primary)
                        .frame(width: max(2, geo.size.width * fraction))
                }
                .frame(height: 4)
                .contentShape(Rectangle())
                .onTapGesture { location in
                    let seekFraction = location.x / max(1, geo.size.width)
                    player.seek(to: seekFraction * player.duration)
                }
            }
            .frame(width: 80, height: 4)
        }
        .padding(.horizontal, 16)
        .frame(height: 56)
        .glassEffect(.regular, in: .capsule)
    }

    var currentSpeakerOrTimestamp: String {
        if player.transcriptAvailability == .available {
            let currentSeg = player.transcriptSegments
                .filter { $0.start <= player.currentTime }
                .last
            if let seg = currentSeg, !seg.speaker.isEmpty {
                return seg.speaker
            }
        }
        return formatTimestamp(player.currentTime)
    }

    // MARK: - Clipping FAB

    var clipFab: some View {
        VStack(spacing: 4) {
            Button {
                handleFabTap()
            } label: {
                ZStack {
                    Circle()
                        .fill(clipArmed ? Color.primary : Color.highlighterAccent)
                        .frame(width: 56, height: 56)
                    Image(systemName: "pencil")
                        .font(.system(size: 18, weight: .semibold))
                        .foregroundStyle(clipArmed ? Color(.systemBackground) : .white)
                }
            }
            .buttonStyle(.plain)

            Text(fabLabel)
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(.secondary)
        }
    }

    var fabLabel: String {
        if !clipArmed { return "CLIP" }
        if clipRangeStart == nil { return "PICK START" }
        return "PICK END"
    }

    func handleFabTap() {
        if !clipArmed {
            clipArmed = true
            clipRangeStart = nil
            clipRangeEnd = nil
            return
        }
        if clipRangeStart == nil {
            clipRangeStart = player.currentTime
            return
        }
        let end = player.currentTime
        let start = clipRangeStart ?? 0
        player.setClipStart(start)
        player.setClipEnd(end)
        clipRangeEnd = end
        clipArmed = false
        showComposer = true
    }

    // MARK: - Helpers

    func loadClips() async {
        guard let artifact = player.currentArtifact else { return }
        let guid = artifact.preview.podcastItemGuid
        let tagValue = guid.isEmpty
            ? artifact.shareEventId
            : "podcast:item:guid:\(guid)"
        if let clips = try? await app.safeCore.getHighlightsForReference(
            tagName: "i",
            tagValue: tagValue,
            limit: 128
        ) {
            memberClips = clips.sorted { ($0.clipStartSeconds ?? 0) < ($1.clipStartSeconds ?? 0) }
        }
    }
}

