import PhotosUI
import SwiftUI
import UIKit

// MARK: - ChangePhotoSheet
//
// Action-sheet-style chooser per identity-05-synthesis §4.4. Three entries:
// "Choose a style" (curated 6), "Choose from library" (PhotosPicker → Blossom
// upload), and "Paste image URL". Camera capture lands in a later brief.

struct ChangePhotoSheet: View {

    @Binding var pictureURL: String
    @Environment(\.dismiss) private var dismiss
    @Environment(UserIdentityStore.self) private var identity

    @State private var pasteURL: String = ""
    @State private var pasteVisible = false
    @State private var photoItem: PhotosPickerItem?
    @State private var uploadState: UploadState = .idle

    private let uploader: any BlossomUploading

    init(pictureURL: Binding<String>, uploader: any BlossomUploading = BlossomUploader()) {
        self._pictureURL = pictureURL
        self.uploader = uploader
    }

    private enum UploadState: Equatable {
        case idle
        case loading
        case failed(String)

        var isLoading: Bool { self == .loading }
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    NavigationLink {
                        AvatarStylePickerView(pictureURL: $pictureURL)
                    } label: {
                        Label("Choose a style", systemImage: "circle.grid.3x3.fill")
                    }
                    .disabled(uploadState.isLoading)

                    PhotosPicker(selection: $photoItem, matching: .images, photoLibrary: .shared()) {
                        Label("Choose from library", systemImage: "photo.on.rectangle")
                    }
                    .disabled(uploadState.isLoading)

                    Button {
                        pasteVisible.toggle()
                    } label: {
                        Label("Paste image URL", systemImage: "link")
                    }
                    .disabled(uploadState.isLoading)

                    if pasteVisible {
                        TextField("https://…", text: $pasteURL)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .keyboardType(.URL)
                        Button("Use this URL") {
                            pictureURL = pasteURL.trimmed
                            Haptics.success()
                            dismiss()
                        }
                        .disabled(!isValidURL(pasteURL) || uploadState.isLoading)
                    }
                }

                if uploadState.isLoading {
                    Section {
                        HStack(spacing: 10) {
                            ProgressView()
                            Text("Uploading photo…").foregroundStyle(.secondary)
                        }
                    }
                } else if case .failed(let message) = uploadState {
                    Section {
                        Label(message, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.orange)
                    }
                }
            }
            .navigationTitle("Change photo")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .disabled(uploadState.isLoading)
                }
            }
            .onChange(of: photoItem) { _, newItem in
                guard let newItem else { return }
                Task { await handlePicked(newItem) }
            }
        }
    }

    private func handlePicked(_ item: PhotosPickerItem) async {
        uploadState = .loading
        defer { photoItem = nil }
        do {
            guard let raw = try await item.loadTransferable(type: Data.self) else {
                uploadState = .failed("Could not read the selected photo.")
                return
            }
            guard let signer = identity.signer else {
                uploadState = .failed("Sign in to upload a photo.")
                return
            }
            let prepared = try Self.resizeJPEG(raw, maxEdge: 800, quality: 0.85)
            let url = try await uploader.upload(
                data: prepared,
                contentType: "image/jpeg",
                signer: signer
            )
            pictureURL = url.absoluteString
            Haptics.success()
            dismiss()
        } catch {
            uploadState = .failed(error.localizedDescription)
        }
    }

    /// Decode → fit-inside `maxEdge` → JPEG-encode at `quality`. Profile
    /// photos at 4K are absurd and many Blossom servers reject oversized
    /// blobs outright.
    private static func resizeJPEG(_ data: Data, maxEdge: CGFloat, quality: CGFloat) throws -> Data {
        guard let image = UIImage(data: data) else {
            throw ChangePhotoError.unreadablePhoto
        }
        let size = image.size
        let scale = min(1, maxEdge / max(size.width, size.height))
        let target = CGSize(width: size.width * scale, height: size.height * scale)
        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1
        format.opaque = true
        let renderer = UIGraphicsImageRenderer(size: target, format: format)
        let resized = renderer.image { _ in
            image.draw(in: CGRect(origin: .zero, size: target))
        }
        guard let jpeg = resized.jpegData(compressionQuality: quality) else {
            throw ChangePhotoError.encodingFailed
        }
        return jpeg
    }

    private func isValidURL(_ s: String) -> Bool {
        guard let url = URL(string: s.trimmed),
              let scheme = url.scheme?.lowercased() else { return false }
        return scheme == "http" || scheme == "https"
    }
}

private enum ChangePhotoError: LocalizedError {
    case unreadablePhoto
    case encodingFailed

    var errorDescription: String? {
        switch self {
        case .unreadablePhoto: return "Could not read the selected photo."
        case .encodingFailed: return "Could not prepare the photo for upload."
        }
    }
}
