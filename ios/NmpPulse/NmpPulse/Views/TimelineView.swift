import SwiftUI

/// Live kind:1 feed from the kernel-bootstrap pubkey (see
/// `crates/nmp-core/src/relay.rs::TEST_PUBKEY`). Pulse currently does NOT
/// drive `nmp_app_signin_*`, so the timeline is whatever the kernel
/// auto-loads on start — typically the bootstrap pubkey's recent notes.
///
/// Filed as T66a: replace bootstrap-pubkey timeline with active-account
/// FollowingTimeline once the sign-in FFI lands.
struct TimelineView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var showCompose = false

    var body: some View {
        ZStack(alignment: .bottomTrailing) {
            Group {
                if model.items.isEmpty {
                    placeholder
                } else {
                    List(model.items) { item in
                        NavigationLink(value: item) {
                            NoteRow(item: item)
                        }
                    }
                    .listStyle(.plain)
                }
            }

            Button {
                showCompose = true
            } label: {
                Image(systemName: "square.and.pencil")
                    .font(.title2)
                    .padding(18)
                    .background(Circle().fill(.tint))
                    .foregroundStyle(.white)
                    .shadow(radius: 4)
            }
            .padding(20)
            .accessibilityLabel("Compose")
        }
        .navigationTitle("Pulse")
        .navigationBarTitleDisplayMode(.large)
        .navigationDestination(for: TimelineItem.self) { item in
            NoteDetailView(rootItem: item)
        }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Text("rev \(model.rev)")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
        .sheet(isPresented: $showCompose) {
            ComposeView()
        }
    }

    private var placeholder: some View {
        VStack(spacing: 16) {
            ProgressView()
            Text("Waiting for kernel snapshot…")
                .foregroundStyle(.secondary)
            Text("Bootstrap pubkey: \(model.testNpub)")
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(.horizontal)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct NoteRow: View {
    let item: TimelineItem

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                AvatarCircle(initials: item.authorAvatarInitials, colorHex: item.authorAvatarColor)
                VStack(alignment: .leading) {
                    Text(item.authorDisplay)
                        .font(.subheadline)
                        .bold()
                    Text(item.createdAtDisplay)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            Text(item.contentPreview.isEmpty ? item.content : item.contentPreview)
                .font(.body)
                .lineLimit(8)
        }
        .padding(.vertical, 4)
    }
}

private struct AvatarCircle: View {
    let initials: String
    let colorHex: String

    var body: some View {
        ZStack {
            Circle()
                .fill(Color(hex: colorHex) ?? .gray)
                .frame(width: 36, height: 36)
            Text(initials)
                .font(.caption)
                .bold()
                .foregroundColor(.white)
        }
    }
}

private extension Color {
    /// Decode a `#RRGGBB` or `RRGGBB` hex string. Returns `nil` on malformed
    /// input — caller falls back to a default.
    init?(hex: String) {
        var clean = hex.trimmingCharacters(in: .whitespacesAndNewlines)
        if clean.hasPrefix("#") { clean.removeFirst() }
        guard clean.count == 6, let value = UInt32(clean, radix: 16) else {
            return nil
        }
        let r = Double((value >> 16) & 0xFF) / 255.0
        let g = Double((value >> 8) & 0xFF) / 255.0
        let b = Double(value & 0xFF) / 255.0
        self = Color(red: r, green: g, blue: b)
    }
}
