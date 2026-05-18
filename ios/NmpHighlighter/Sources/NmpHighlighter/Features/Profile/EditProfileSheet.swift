import Kingfisher
import PhotosUI
import SwiftUI

/// Edit-profile flow for the current user. Pre-populates from the cached
/// `ProfileMetadata` so a user who's never published a kind:0 still gets
/// blank fields, and a user who has gets their values back exactly as
/// they were last seen on a relay (`unknown_field` round-trip is handled
/// at the Rust layer — `publish_profile` preserves keys we don't know).
///
/// Picture and banner uploads route through Blossom via the same
/// `safeCore.uploadPhoto` path the rooms / capture flows use; we stash
/// the returned URL into the corresponding text field so the form can
/// show progress + the user can still paste a URL by hand if they
/// prefer not to re-upload.
struct EditProfileSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(HighlighterStore.self) private var appStore

    let initial: ProfileMetadata?
    let onSaved: (ProfileMetadata) -> Void

    @State private var displayName: String = ""
    @State private var name: String = ""
    @State private var about: String = ""
    @State private var picture: String = ""
    @State private var banner: String = ""
    @State private var nip05: String = ""
    @State private var website: String = ""
    @State private var lud16: String = ""

    @State private var pictureItem: PhotosPickerItem?
    @State private var bannerItem: PhotosPickerItem?
    @State private var pictureUploading = false
    @State private var bannerUploading = false

    @State private var saving = false
    @State private var error: String?

    private var isDirty: Bool {
        displayName != (initial?.displayName ?? "")
            || name != (initial?.name ?? "")
            || about != (initial?.about ?? "")
            || picture != (initial?.picture ?? "")
            || banner != (initial?.banner ?? "")
            || nip05 != (initial?.nip05 ?? "")
            || website != (initial?.website ?? "")
            || lud16 != (initial?.lud16 ?? "")
    }

    var body: some View {
        NavigationStack {
            ZStack(alignment: .bottom) {
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        bannerPlate
                        avatarPlate
                            .padding(.horizontal, 22)
                            .padding(.top, -52) // overlap the banner
                        identityFields
                            .padding(.horizontal, 22)
                        Divider().overlay(Color.highlighterRule)
                            .padding(.horizontal, 22)
                        contactFields
                            .padding(.horizontal, 22)
                        Spacer(minLength: 120)
                    }
                }
                .scrollDismissesKeyboard(.interactively)

                stickyCTA
            }
            .background(Color.highlighterPaper.ignoresSafeArea())
            .navigationTitle("Edit profile")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                        .foregroundStyle(Color.highlighterInkStrong)
                }
            }
            .alert("Couldn't save", isPresented: errorBinding) {
                Button("OK") { error = nil }
            } message: {
                if let error { Text(error) }
            }
            .onAppear { hydrate() }
            .onChange(of: pictureItem) { _, item in
                guard let item else { return }
                Task { await upload(item: item, into: \.picture, uploading: \.pictureUploading) }
            }
            .onChange(of: bannerItem) { _, item in
                guard let item else { return }
                Task { await upload(item: item, into: \.banner, uploading: \.bannerUploading) }
            }
        }
    }

    // MARK: - Sections

    private var bannerPlate: some View {
        ZStack {
            if let url = URL(string: banner), !banner.isEmpty {
                KFImage(url)
                    .resizable()
                    .scaledToFill()
                    .frame(maxWidth: .infinity)
                    .frame(height: 160)
                    .clipped()
            } else {
                LinearGradient(
                    colors: [
                        Color.highlighterAccent.opacity(0.55),
                        Color.highlighterAccent.opacity(0.18),
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
                .frame(height: 160)
            }
        }
        .overlay(alignment: .topTrailing) {
            if !banner.isEmpty {
                clearChip { banner = ""; bannerItem = nil }
                    .padding(12)
            }
        }
        .overlay(alignment: .bottomTrailing) {
            HStack(spacing: 6) {
                if bannerUploading {
                    ProgressView().controlSize(.small).tint(.white)
                }
                PhotosPicker(selection: $bannerItem, matching: .images) {
                    Label(banner.isEmpty ? "Add banner" : "Replace", systemImage: "photo")
                        .labelStyle(.titleAndIcon)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(.black.opacity(0.55), in: Capsule())
                }
            }
            .padding(12)
        }
    }

    private var avatarPlate: some View {
        HStack(spacing: 14) {
            ZStack {
                if let url = URL(string: picture), !picture.isEmpty {
                    KFImage(url)
                        .resizable()
                        .scaledToFill()
                } else {
                    LinearGradient(
                        colors: [
                            Color.highlighterTintPale,
                            Color.highlighterAccent.opacity(0.4),
                        ],
                        startPoint: .top,
                        endPoint: .bottom
                    )
                    .overlay {
                        Image(systemName: "person.fill")
                            .font(.system(size: 36))
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                }
            }
            .frame(width: 96, height: 96)
            .clipShape(Circle())
            .overlay(Circle().stroke(Color.highlighterPaper, lineWidth: 4))
            .overlay {
                if pictureUploading {
                    Circle().fill(.black.opacity(0.4))
                    ProgressView().controlSize(.regular).tint(.white)
                }
            }

            VStack(alignment: .leading, spacing: 8) {
                PhotosPicker(selection: $pictureItem, matching: .images) {
                    Label(picture.isEmpty ? "Add photo" : "Replace photo", systemImage: "camera")
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(Color.highlighterInkStrong)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(
                            Capsule().fill(Color.highlighterTintPale)
                        )
                }
                if !picture.isEmpty {
                    Button {
                        picture = ""
                        pictureItem = nil
                    } label: {
                        Text("Remove")
                            .font(.subheadline)
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                }
            }
            Spacer(minLength: 0)
        }
    }

    private var identityFields: some View {
        VStack(alignment: .leading, spacing: 18) {
            field(
                label: "Display name",
                placeholder: "How you want to be addressed",
                text: $displayName
            )
            field(
                label: "Username",
                placeholder: "lowercase, no spaces",
                text: $name,
                autocap: .never,
                autocorrect: false
            )
            VStack(alignment: .leading, spacing: 6) {
                fieldLabel("About")
                TextField(
                    "",
                    text: $about,
                    prompt: Text("A line or two — what do you read?")
                        .foregroundColor(Color.highlighterInkMuted.opacity(0.7)),
                    axis: .vertical
                )
                .font(.body)
                .foregroundStyle(Color.highlighterInkStrong)
                .lineLimit(3...8)
            }
        }
    }

    private var contactFields: some View {
        VStack(alignment: .leading, spacing: 18) {
            field(
                label: "NIP-05",
                placeholder: "you@example.com",
                text: $nip05,
                autocap: .never,
                keyboard: .emailAddress,
                autocorrect: false
            )
            field(
                label: "Website",
                placeholder: "https://…",
                text: $website,
                autocap: .never,
                keyboard: .URL,
                autocorrect: false
            )
            field(
                label: "Lightning address",
                placeholder: "you@walletofsatoshi.com",
                text: $lud16,
                autocap: .never,
                keyboard: .emailAddress,
                autocorrect: false
            )
        }
    }

    private func field(
        label: String,
        placeholder: String,
        text: Binding<String>,
        autocap: TextInputAutocapitalization = .sentences,
        keyboard: UIKeyboardType = .default,
        autocorrect: Bool = true
    ) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            fieldLabel(label)
            TextField(
                "",
                text: text,
                prompt: Text(placeholder)
                    .foregroundColor(Color.highlighterInkMuted.opacity(0.7))
            )
            .font(.body)
            .foregroundStyle(Color.highlighterInkStrong)
            .textInputAutocapitalization(autocap)
            .autocorrectionDisabled(!autocorrect)
            .keyboardType(keyboard)
        }
    }

    private func fieldLabel(_ text: String) -> some View {
        Text(text.uppercased())
            .font(.footnote.weight(.semibold))
            .tracking(0.6)
            .foregroundStyle(Color.highlighterInkMuted)
    }

    private func clearChip(action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: "xmark")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.white)
                .padding(8)
                .background(.black.opacity(0.55), in: Circle())
        }
    }

    private var stickyCTA: some View {
        VStack(spacing: 0) {
            LinearGradient(
                colors: [Color.highlighterPaper.opacity(0), Color.highlighterPaper],
                startPoint: .top,
                endPoint: .bottom
            )
            .frame(height: 24)

            Button(action: save) {
                ZStack {
                    if saving {
                        ProgressView().tint(.white)
                    } else {
                        Text("Save")
                            .font(.headline)
                            .foregroundStyle(.white)
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 16)
                .background(
                    RoundedRectangle(cornerRadius: 16)
                        .fill(canSave ? Color.highlighterAccent : Color.highlighterAccent.opacity(0.35))
                )
            }
            .buttonStyle(.plain)
            .disabled(!canSave)
            .padding(.horizontal, 22)
            .padding(.bottom, 24)
            .background(Color.highlighterPaper)
        }
    }

    // MARK: - Helpers

    private var canSave: Bool {
        isDirty && !saving && !pictureUploading && !bannerUploading
    }

    private var errorBinding: Binding<Bool> {
        Binding(get: { error != nil }, set: { if !$0 { error = nil } })
    }

    private func hydrate() {
        guard let p = initial else { return }
        displayName = p.displayName
        name = p.name
        about = p.about
        picture = p.picture
        banner = p.banner
        nip05 = p.nip05
        website = p.website
        lud16 = p.lud16
    }

    private func upload(
        item: PhotosPickerItem,
        into keyPath: ReferenceWritableKeyPath<EditProfileSheet, String>,
        uploading uploadingKey: ReferenceWritableKeyPath<EditProfileSheet, Bool>
    ) async {
        // SwiftUI structs can't have ReferenceWritableKeyPath into State —
        // do the inline assignment via the closure pattern instead.
        await runUpload(item: item) { url in
            switch keyPath {
            case \EditProfileSheet.picture: picture = url
            case \EditProfileSheet.banner: banner = url
            default: break
            }
        } uploading: { isUploading in
            switch uploadingKey {
            case \EditProfileSheet.pictureUploading: pictureUploading = isUploading
            case \EditProfileSheet.bannerUploading: bannerUploading = isUploading
            default: break
            }
        }
    }

    private func runUpload(
        item: PhotosPickerItem,
        commit: @escaping (String) -> Void,
        uploading: @escaping (Bool) -> Void
    ) async {
        uploading(true)
        defer { uploading(false) }
        do {
            guard let data = try await item.loadTransferable(type: Data.self),
                  let image = UIImage(data: data) else {
                error = "Couldn't read that image."
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
            commit(upload.url)
        } catch {
            self.error = "Upload failed: \(error.localizedDescription)"
        }
    }

    private struct PreparedImage { let data: Data; let width: Int; let height: Int }

    private func prepareForUpload(image: UIImage) async -> PreparedImage {
        let maxSide: CGFloat = 1600
        let scale = min(1, maxSide / max(image.size.width, image.size.height))
        let target = CGSize(width: image.size.width * scale, height: image.size.height * scale)
        let renderer = UIGraphicsImageRenderer(size: target)
        let scaled = renderer.image { _ in image.draw(in: CGRect(origin: .zero, size: target)) }
        let data = scaled.jpegData(compressionQuality: 0.85) ?? Data()
        return PreparedImage(data: data, width: Int(scaled.size.width), height: Int(scaled.size.height))
    }

    private func save() {
        guard canSave else { return }
        saving = true
        Task {
            defer { Task { @MainActor in saving = false } }
            do {
                let updated = try await appStore.safeCore.updateProfile(
                    name: name.trimmingCharacters(in: .whitespacesAndNewlines),
                    displayName: displayName.trimmingCharacters(in: .whitespacesAndNewlines),
                    about: about.trimmingCharacters(in: .whitespacesAndNewlines),
                    picture: picture.trimmingCharacters(in: .whitespacesAndNewlines),
                    banner: banner.trimmingCharacters(in: .whitespacesAndNewlines),
                    nip05: nip05.trimmingCharacters(in: .whitespacesAndNewlines),
                    website: website.trimmingCharacters(in: .whitespacesAndNewlines),
                    lud16: lud16.trimmingCharacters(in: .whitespacesAndNewlines)
                )
                await MainActor.run {
                    UINotificationFeedbackGenerator().notificationOccurred(.success)
                    onSaved(updated)
                    dismiss()
                }
            } catch {
                await MainActor.run {
                    self.error = error.localizedDescription
                }
            }
        }
    }
}
