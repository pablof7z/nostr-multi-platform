import Kingfisher
import PhotosUI
import SwiftUI

/// Founding-a-room flow. One `.large` sheet, no wizard. The name field is
/// the only serif on the screen — the room is being given an identity, and
/// the typeface honours that. Visibility is an inline row, not a segmented
/// control — the default (public · open) is sane, so most users never
/// touch it. Picking a cover routes through `PhotosPicker` →
/// `core.uploadPhoto` so the image lands on the user's Blossom server
/// before the room is created. On success, pushes `RoomInviteView` in
/// welcome mode so adding the first guests feels like one continuous act.
struct CreateRoomSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(HighlighterStore.self) private var appStore

    @State private var name: String = ""
    @State private var about: String = ""
    @State private var visibility: RoomVisibility = .public
    @State private var access: RoomAccess = .open
    @State private var visibilityPickerPresented = false
    @State private var photoItem: PhotosPickerItem?
    @State private var coverUpload: BlossomUpload?
    @State private var coverIsUploading = false
    @State private var isCreating = false
    @State private var error: String?
    @State private var createdGroupId: String?

    @FocusState private var focused: Field?
    private enum Field { case name, about }

    private var canCreate: Bool {
        name.trimmingCharacters(in: .whitespacesAndNewlines).count >= 2
            && !isCreating
            && !coverIsUploading
    }

    var body: some View {
        NavigationStack {
            ZStack(alignment: .bottom) {
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        coverPlate
                        identityFields
                            .padding(.horizontal, 22)
                        Divider().overlay(Color.highlighterRule)
                            .padding(.horizontal, 22)
                        visibilityRow
                            .padding(.horizontal, 22)
                        Spacer(minLength: 120)
                    }
                    .padding(.top, 8)
                }
                .scrollDismissesKeyboard(.interactively)

                stickyCTA
            }
            .background(Color.highlighterPaper.ignoresSafeArea())
            .navigationTitle("")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                        .foregroundStyle(Color.highlighterInkStrong)
                }
            }
            .navigationDestination(item: $createdGroupId) { groupId in
                RoomInviteView(groupId: groupId, mode: .welcome) {
                    dismiss()
                }
            }
            .sheet(isPresented: $visibilityPickerPresented) {
                VisibilityPickerSheet(
                    visibility: $visibility,
                    access: $access
                )
                .presentationDetents([.medium])
                .presentationDragIndicator(.visible)
            }
            .alert("Couldn't create room", isPresented: errorBinding, actions: {
                Button("OK") { error = nil }
            }, message: {
                if let error { Text(error) }
            })
            .onChange(of: photoItem) { _, newItem in
                guard let newItem else { return }
                Task { await uploadCover(item: newItem) }
            }
        }
    }

    // MARK: - Sections

    private var coverPlate: some View {
        ZStack {
            if let upload = coverUpload, let url = URL(string: upload.url) {
                KFImage(url)
                    .resizable()
                    .scaledToFill()
                    .frame(maxWidth: .infinity)
                    .frame(height: 200)
                    .clipped()
            } else {
                LinearGradient(
                    colors: [
                        Color.highlighterAccent.opacity(0.85),
                        Color.highlighterAccent.opacity(0.45),
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
                .frame(height: 200)
                .overlay {
                    if coverIsUploading {
                        ProgressView()
                            .tint(.white)
                            .controlSize(.large)
                    } else {
                        VStack(spacing: 8) {
                            Image(systemName: "photo.on.rectangle.angled")
                                .font(.title)
                                .foregroundStyle(.white.opacity(0.92))
                            Text("Add a cover")
                                .font(.subheadline.weight(.medium))
                                .foregroundStyle(.white.opacity(0.92))
                        }
                    }
                }
            }
        }
        .overlay(alignment: .topTrailing) {
            if coverUpload != nil {
                Button {
                    coverUpload = nil
                    photoItem = nil
                } label: {
                    Image(systemName: "xmark")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white)
                        .padding(8)
                        .background(.black.opacity(0.45), in: Circle())
                }
                .padding(12)
            }
        }
        .overlay {
            // Make the whole plate tappable to pick — no fiddly tiny buttons.
            PhotosPicker(selection: $photoItem, matching: .images) {
                Color.clear
            }
            .buttonStyle(.plain)
        }
    }

    private var identityFields: some View {
        VStack(alignment: .leading, spacing: 18) {
            TextField(
                "",
                text: $name,
                prompt: Text("Name your room").foregroundColor(Color.highlighterInkMuted.opacity(0.7))
            )
            .font(.system(.largeTitle, design: .default).weight(.semibold))
            .foregroundStyle(Color.highlighterInkStrong)
            .focused($focused, equals: .name)
            .submitLabel(.next)
            .onSubmit { focused = .about }
            .lineLimit(2)

            TextField(
                "",
                text: $about,
                prompt: Text("What will you read together?")
                    .foregroundColor(Color.highlighterInkMuted.opacity(0.7)),
                axis: .vertical
            )
            .font(.body)
            .foregroundStyle(Color.highlighterInkStrong)
            .focused($focused, equals: .about)
            .lineLimit(3...8)
        }
    }

    private var visibilityRow: some View {
        Button {
            visibilityPickerPresented = true
        } label: {
            HStack(spacing: 12) {
                Image(systemName: visibilityGlyph)
                    .font(.body.weight(.medium))
                    .foregroundStyle(Color.highlighterAccent)
                    .frame(width: 22)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Visibility")
                        .font(.footnote.weight(.semibold))
                        .tracking(0.6)
                        .foregroundStyle(Color.highlighterInkMuted)
                    Text(visibilitySummary)
                        .font(.body.weight(.medium))
                        .foregroundStyle(Color.highlighterInkStrong)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkMuted)
            }
            .padding(.vertical, 6)
        }
        .buttonStyle(.plain)
    }

    private var stickyCTA: some View {
        VStack(spacing: 0) {
            LinearGradient(
                colors: [Color.highlighterPaper.opacity(0), Color.highlighterPaper],
                startPoint: .top,
                endPoint: .bottom
            )
            .frame(height: 24)

            Button(action: create) {
                ZStack {
                    if isCreating {
                        ProgressView().tint(.white)
                    } else {
                        Text("Create Room")
                            .font(.headline)
                            .foregroundStyle(.white)
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 16)
                .background(
                    RoundedRectangle(cornerRadius: 16)
                        .fill(canCreate ? Color.highlighterAccent : Color.highlighterAccent.opacity(0.35))
                )
            }
            .buttonStyle(.plain)
            .disabled(!canCreate)
            .padding(.horizontal, 22)
            .padding(.bottom, 24)
            .background(Color.highlighterPaper)
        }
    }

    // MARK: - Helpers

    private var visibilityGlyph: String {
        switch (visibility, access) {
        case (.public, .open): return "globe"
        case (.public, .closed): return "globe.badge.chevron.backward"
        case (.private, _): return "lock"
        }
    }

    private var visibilitySummary: String {
        switch (visibility, access) {
        case (.public, .open): return "Public · Anyone can join"
        case (.public, .closed): return "Public · You approve joins"
        case (.private, _): return "Private · Invite only"
        }
    }

    private var errorBinding: Binding<Bool> {
        Binding(get: { error != nil }, set: { if !$0 { error = nil } })
    }

    private func uploadCover(item: PhotosPickerItem) async {
        coverIsUploading = true
        defer { coverIsUploading = false }
        do {
            guard let data = try await item.loadTransferable(type: Data.self) else { return }
            guard let image = UIImage(data: data) else {
                error = "That image couldn't be read."
                return
            }
            let prepared = await prepareForUpload(image: image)
            let upload = try await appStore.safeCore.uploadPhoto(
                bytes: prepared.data,
                mime: "image/jpeg",
                width: UInt32(prepared.width),
                height: UInt32(prepared.height),
                alt: ""
            )
            coverUpload = upload
        } catch {
            self.error = "Couldn't upload cover: \(error.localizedDescription)"
        }
    }

    private struct PreparedImage {
        let data: Data
        let width: Int
        let height: Int
    }

    private func prepareForUpload(image: UIImage) async -> PreparedImage {
        let maxSide: CGFloat = 1600
        let scale = min(1, maxSide / max(image.size.width, image.size.height))
        let target = CGSize(width: image.size.width * scale, height: image.size.height * scale)
        let renderer = UIGraphicsImageRenderer(size: target)
        let scaled = renderer.image { _ in
            image.draw(in: CGRect(origin: .zero, size: target))
        }
        let data = scaled.jpegData(compressionQuality: 0.85) ?? Data()
        return PreparedImage(
            data: data,
            width: Int(scaled.size.width),
            height: Int(scaled.size.height)
        )
    }

    private func create() {
        guard canCreate else { return }
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedAbout = about.trimmingCharacters(in: .whitespacesAndNewlines)
        let pictureURL = coverUpload?.url ?? ""
        isCreating = true
        focused = nil
        Task {
            defer { isCreating = false }
            do {
                let groupId = try await appStore.safeCore.createRoom(
                    name: trimmedName,
                    about: trimmedAbout,
                    picture: pictureURL,
                    visibility: visibility,
                    access: access
                )
                let generator = UINotificationFeedbackGenerator()
                generator.notificationOccurred(.success)
                createdGroupId = groupId
            } catch {
                self.error = "Couldn't publish: \(error.localizedDescription)"
            }
        }
    }
}

extension String: @retroactive Identifiable {
    public var id: String { self }
}

// MARK: - Visibility picker

private struct VisibilityPickerSheet: View {
    @Binding var visibility: RoomVisibility
    @Binding var access: RoomAccess
    @Environment(\.dismiss) private var dismiss

    private struct Option: Identifiable {
        let id: String
        let title: String
        let summary: String
        let glyph: String
        let visibility: RoomVisibility
        let access: RoomAccess
    }

    private let options: [Option] = [
        Option(
            id: "public-open",
            title: "Public",
            summary: "Anyone can find and join this room.",
            glyph: "globe",
            visibility: .public,
            access: .open
        ),
        Option(
            id: "public-closed",
            title: "Public · By approval",
            summary: "Anyone can find it, but you approve who joins.",
            glyph: "globe.badge.chevron.backward",
            visibility: .public,
            access: .closed
        ),
        Option(
            id: "private",
            title: "Private",
            summary: "Hidden from the explorer. Invite only.",
            glyph: "lock",
            visibility: .private,
            access: .closed
        ),
    ]

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(options) { option in
                        Button {
                            visibility = option.visibility
                            access = option.access
                            UISelectionFeedbackGenerator().selectionChanged()
                            dismiss()
                        } label: {
                            HStack(alignment: .top, spacing: 14) {
                                Image(systemName: option.glyph)
                                    .font(.title3)
                                    .foregroundStyle(Color.highlighterAccent)
                                    .frame(width: 28)
                                    .padding(.top, 2)
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(option.title)
                                        .font(.body.weight(.semibold))
                                        .foregroundStyle(Color.highlighterInkStrong)
                                    Text(option.summary)
                                        .font(.subheadline)
                                        .foregroundStyle(Color.highlighterInkMuted)
                                        .multilineTextAlignment(.leading)
                                }
                                Spacer(minLength: 0)
                                if isSelected(option) {
                                    Image(systemName: "checkmark")
                                        .font(.body.weight(.semibold))
                                        .foregroundStyle(Color.highlighterAccent)
                                }
                            }
                            .padding(.horizontal, 22)
                            .padding(.vertical, 16)
                        }
                        .buttonStyle(.plain)
                        if option.id != options.last?.id {
                            Divider().overlay(Color.highlighterRule)
                                .padding(.leading, 64)
                        }
                    }
                }
            }
            .background(Color.highlighterPaper.ignoresSafeArea())
            .navigationTitle("Visibility")
            .navigationBarTitleDisplayMode(.inline)
        }
    }

    private func isSelected(_ option: VisibilityPickerSheet.Option) -> Bool {
        option.visibility == visibility
            && (option.visibility == .private || option.access == access)
    }
}
