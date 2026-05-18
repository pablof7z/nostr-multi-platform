import SwiftUI

struct ClipComposerSheet: View {
    @Environment(HighlighterStore.self) private var app
    @Environment(\.dismiss) private var dismiss

    let artifact: ArtifactRecord

    @Binding var startSeconds: Double
    @Binding var endSeconds: Double

    @State private var note: String = ""
    @State private var selectedGroupId: String?
    @State private var showCommunityPicker = false
    @State private var isPublishing = false
    @State private var publishError: String?

    private var player: PodcastPlayerStore { app.podcastPlayer }

    // MARK: - Computed

    private var duration: Double { endSeconds - startSeconds }

    private var matchingSegments: [TranscriptSegment] {
        guard player.transcriptAvailability == .available else { return [] }
        return player.transcriptSegments.filter { seg in
            seg.start < endSeconds && seg.end > startSeconds
        }
    }

    private var extractedFragment: String {
        matchingSegments.map(\.text).joined(separator: " ")
    }

    private var inferredSpeaker: String {
        matchingSegments.first(where: { !$0.speaker.isEmpty })?.speaker ?? ""
    }

    private var durationLabel: String {
        let total = Int(duration)
        let m = total / 60
        let s = total % 60
        if m > 0 { return "\(m)m \(s)s" }
        return "\(s)s"
    }

    private var subtitleLabel: String {
        let dl = durationLabel
        return matchingSegments.isEmpty ? "\(dl) · time-only clip" : "\(dl) · with transcript"
    }

    private var canPublish: Bool {
        startSeconds >= 0
            && endSeconds <= player.duration
            && startSeconds + 5 <= endSeconds
            && !isPublishing
    }

    private var communityName: String {
        guard let id = selectedGroupId else { return "" }
        if let community = app.joinedCommunities.first(where: { $0.id == id }) {
            return community.name.isEmpty ? id : community.name
        }
        return id
    }

    // MARK: - Body

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                header
                    .padding(.horizontal, 20)
                    .padding(.top, 4)
                    .padding(.bottom, 16)

                Divider()

                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        excerptSlot
                        noteField
                        roomPickerRow
                        if let err = publishError {
                            Text(err)
                                .font(.footnote)
                                .foregroundStyle(.red)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        actionsRow
                    }
                    .padding(.horizontal, 20)
                    .padding(.vertical, 16)
                }
            }
            .presentationDetents([.medium, .large])
            .presentationDragIndicator(.visible)
            .sheet(isPresented: $showCommunityPicker) {
                CommunityPicker(selection: $selectedGroupId)
                    .environment(app)
            }
        }
        .onAppear {
            if selectedGroupId == nil && !artifact.groupId.isEmpty {
                selectedGroupId = artifact.groupId
            }
        }
    }

    // MARK: - Header

    private var header: some View {
        VStack(spacing: 12) {
            Text("New Clip")
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .center)

            rangeRow
        }
    }

    private var rangeRow: some View {
        VStack(spacing: 6) {
            HStack(spacing: 0) {
                timeEditor(seconds: $startSeconds, direction: .leading) { delta in
                    let proposed = startSeconds + delta
                    startSeconds = max(0, min(endSeconds - 5, proposed))
                }

                Spacer(minLength: 0)

                Text("→")
                    .font(.title3.weight(.light))
                    .foregroundStyle(.secondary)

                Spacer(minLength: 0)

                timeEditor(seconds: $endSeconds, direction: .trailing) { delta in
                    let proposed = endSeconds + delta
                    endSeconds = max(startSeconds + 5, min(player.duration > 0 ? player.duration : proposed, proposed))
                }
            }

            Text(subtitleLabel)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .center)
        }
    }

    private enum NudgeAlignment { case leading, trailing }

    private func timeEditor(
        seconds: Binding<Double>,
        direction: NudgeAlignment,
        onNudge: @escaping (Double) -> Void
    ) -> some View {
        HStack(spacing: 8) {
            if direction == .trailing {
                nudgeButton(label: "-5s", delta: -5, onNudge: onNudge)
            }

            Text(formatTimestamp(seconds.wrappedValue))
                .font(.system(size: 24, weight: .semibold).monospacedDigit())
                .foregroundStyle(.primary)

            if direction == .leading {
                nudgeButton(label: "+5s", delta: +5, onNudge: onNudge)
            }
        }
    }

    private func nudgeButton(label: String, delta: Double, onNudge: @escaping (Double) -> Void) -> some View {
        Button {
            onNudge(delta)
        } label: {
            Text(label)
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color(.tertiarySystemFill), in: Capsule())
        }
        .buttonStyle(.plain)
    }

    // MARK: - Excerpt slot

    @ViewBuilder
    private var excerptSlot: some View {
        if !extractedFragment.isEmpty {
            HStack(alignment: .top, spacing: 0) {
                Rectangle()
                    .fill(Color.highlighterAccent)
                    .frame(width: 3)
                    .clipShape(RoundedRectangle(cornerRadius: 2))

                Text(extractedFragment)
                    .font(.system(.callout, design: .default).italic())
                    .foregroundStyle(.primary)
                    .lineSpacing(6)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 10)
            }
            .background(Color(.secondarySystemFill), in: RoundedRectangle(cornerRadius: 8))
        } else {
            VStack(alignment: .leading, spacing: 6) {
                Text("No transcript in range")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)

                Text("Time-only clip · \(durationLabel). Add a note for the room below.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(Color(.tertiarySystemFill), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    // MARK: - Note field

    private var noteField: some View {
        TextField("Add a note for the room…", text: $note, axis: .vertical)
            .lineLimit(1...4)
            .font(.callout)
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 12))
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .strokeBorder(Color(.separator), lineWidth: 1)
            )
    }

    // MARK: - Room picker

    private var roomPickerRow: some View {
        Button {
            showCommunityPicker = true
        } label: {
            HStack(spacing: 10) {
                Image(systemName: "number")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(width: 20)
                Text("Room")
                    .font(.callout)
                    .foregroundStyle(.primary)
                Spacer()
                Text(communityName.isEmpty ? "Personal" : communityName)
                    .font(.callout)
                    .foregroundStyle(communityName.isEmpty ? Color.secondary : Color.highlighterAccent)
                    .lineLimit(1)
                Image(systemName: "chevron.right")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color(.separator), lineWidth: 1))
        }
        .buttonStyle(.plain)
    }

    // MARK: - Actions

    private var actionsRow: some View {
        HStack(spacing: 12) {
            Button("Cancel") {
                dismiss()
            }
            .font(.body.weight(.medium))
            .foregroundStyle(.primary)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 14)
            .background(Color(.secondarySystemFill), in: RoundedRectangle(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Color(.separator), lineWidth: 1))
            .buttonStyle(.plain)

            Button {
                publishClip()
            } label: {
                Group {
                    if isPublishing {
                        ProgressView()
                            .tint(.white)
                    } else {
                        Text("Publish")
                            .font(.body.weight(.semibold))
                            .foregroundStyle(.white)
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 14)
                .background(
                    canPublish ? Color.highlighterAccent : Color.highlighterAccent.opacity(0.4),
                    in: RoundedRectangle(cornerRadius: 14)
                )
            }
            .buttonStyle(.plain)
            .disabled(!canPublish)
        }
    }

    // MARK: - Publish

    private func publishClip() {
        guard canPublish else { return }
        isPublishing = true
        publishError = nil

        let draft = HighlightDraft(
            quote: extractedFragment,
            context: note,
            note: "",
            clipStartSeconds: startSeconds,
            clipEndSeconds: endSeconds,
            clipSpeaker: inferredSpeaker,
            clipTranscriptSegmentIds: matchingSegments.map(\.id),
            image: nil
        )

        Task {
            do {
                if let groupId = selectedGroupId, !groupId.isEmpty {
                    _ = try await app.safeCore.publishHighlightsAndShare(
                        artifact: artifact,
                        drafts: [draft],
                        targetGroupId: groupId
                    )
                } else {
                    _ = try await app.safeCore.publishHighlight(draft: draft, artifact: artifact)
                }
                await MainActor.run {
                    isPublishing = false
                    app.shareToast = "Clip shared"
                    dismiss()
                }
            } catch {
                await MainActor.run {
                    isPublishing = false
                    publishError = error.localizedDescription
                }
            }
        }
    }
}

// MARK: - Timestamp formatter

private func formatTimestamp(_ seconds: Double) -> String {
    guard seconds.isFinite, seconds >= 0 else { return "0:00" }
    let total = Int(seconds)
    let h = total / 3600
    let m = (total % 3600) / 60
    let s = total % 60
    if h > 0 { return String(format: "%d:%02d:%02d", h, m, s) }
    return String(format: "%d:%02d", m, s)
}
