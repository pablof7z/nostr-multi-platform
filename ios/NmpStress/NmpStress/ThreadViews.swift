import SwiftUI

struct TimelineRow: View {
    let item: TimelineItem
    let openAuthor: () -> Void
    let openThread: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Button(action: openAuthor) {
                RemoteAvatar(
                    url: item.authorPictureUrl,
                    initials: item.authorAvatarInitials,
                    color: item.authorAvatarColor,
                    source: item.authorAvatarSource,
                    size: 38
                )
            }
            .buttonStyle(.borderless)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Open profile")
            .accessibilityIdentifier("timeline-author-link")

            VStack(alignment: .leading, spacing: 5) {
                HStack {
                    Button(action: openAuthor) {
                        Text(item.authorDisplay)
                            .font(.subheadline.weight(.semibold))
                            .lineLimit(1)
                            .accessibilityIdentifier("timeline-author-link")
                    }
                    .buttonStyle(.borderless)
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel("Open profile")
                    .accessibilityIdentifier("timeline-author-name-link")
                    Spacer()
                    Label("\(item.relayCount)", systemImage: "antenna.radiowaves.left.and.right")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                Button(action: openThread) {
                    VStack(alignment: .leading, spacing: 5) {
                        Text(item.contentPreview)
                            .font(.footnote)
                            .lineLimit(4)
                            .foregroundStyle(.primary)
                            .multilineTextAlignment(.leading)
                        HStack(spacing: 8) {
                            Text(item.createdAtDisplay)
                                .font(.caption2.monospacedDigit())
                                .foregroundStyle(.secondary)
                            Label("Thread", systemImage: "text.bubble")
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(.secondary)
                                .accessibilityIdentifier("timeline-thread-label")
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .buttonStyle(.borderless)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("Open thread")
                .accessibilityIdentifier("timeline-thread-link")
            }
        }
        .padding(.vertical, 4)
    }
}

struct ThreadDetailView: View {
    @EnvironmentObject private var model: KernelModel
    let eventID: String

    private var view: ThreadViewPayload? {
        if model.threadView?.focusedEventId == eventID {
            return model.threadView
        }
        return model.cachedThreadView(eventID: eventID)
    }

    private var items: [TimelineItem] {
        view?.items ?? model.items.filter { $0.id == eventID }
    }

    private var focusedItem: TimelineItem? {
        items.first { $0.id == eventID }
    }

    private var previousItems: [TimelineItem] {
        guard let index = items.firstIndex(where: { $0.id == eventID }) else {
            return []
        }
        return Array(items.prefix(index))
    }

    private var nextItems: [TimelineItem] {
        guard let index = items.firstIndex(where: { $0.id == eventID }) else {
            return []
        }
        return Array(items.dropFirst(index + 1))
    }

    var body: some View {
        List {
            Section {
                DiagnosticRow("State", view?.state ?? "opening")
                DiagnosticRow("Root", shortPubkeyDisplay(view?.rootEventId ?? eventID))
                DiagnosticRow("Events", "\(items.count)")
                DiagnosticRow("Previous", "\(view?.previousCount ?? previousItems.count)")
                DiagnosticRow("Next", "\(view?.nextCount ?? nextItems.count)")
                    .accessibilityIdentifier("thread-next-count-value")
            }

            if !previousItems.isEmpty {
                Section("Previous Replies") {
                    ForEach(previousItems) { item in
                        ThreadNoteRow(item: item, focused: false)
                    }
                }
            }

            Section("Selected Note") {
                if let focusedItem {
                    ThreadNoteRow(item: focusedItem, focused: true)
                        .accessibilityIdentifier("thread-focused-note")
                } else {
                    Text("Waiting for selected note")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }

            if !nextItems.isEmpty {
                Section("Next Replies") {
                    ForEach(nextItems) { item in
                        ThreadNoteRow(item: item, focused: false)
                    }
                }
            }
        }
        .navigationTitle("Thread")
        .navigationBarTitleDisplayMode(.inline)
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(Color(uiColor: .systemGroupedBackground))
        .task(id: eventID) {
            model.openThread(eventID: eventID)
        }
        .onDisappear {
            model.closeThread(eventID: eventID)
        }
        .accessibilityIdentifier("thread-detail-list")
    }
}

struct ThreadNoteRow: View {
    let item: TimelineItem
    let focused: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            RemoteAvatar(
                url: item.authorPictureUrl,
                initials: item.authorAvatarInitials,
                color: item.authorAvatarColor,
                source: item.authorAvatarSource,
                size: 34
            )
            VStack(alignment: .leading, spacing: 5) {
                HStack {
                    Text(item.authorDisplay)
                        .font(.subheadline.weight(.semibold))
                        .lineLimit(1)
                    Spacer()
                    if focused {
                        Text("selected")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.blue)
                    }
                }
                Text(item.content.isEmpty ? item.contentPreview : item.content)
                    .font(.footnote)
                    .foregroundStyle(.primary)
                    .fixedSize(horizontal: false, vertical: true)
                Text(item.createdAtDisplay)
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
    }
}
