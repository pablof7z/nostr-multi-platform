import SwiftUI

// MARK: - IdentityAvatarView
//
// Verbatim Podcastr IdentityAvatarView requires a user identity/profile image.
// This stub renders the same visual shape (circular avatar with initials fallback).
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Identity/IdentityAvatarView.swift

struct IdentityAvatarView: View {
    let url: URL?
    let initial: Character?
    let size: CGFloat

    var body: some View {
        Group {
            if let url {
                AsyncImage(url: url) { image in
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                } placeholder: {
                    initialsView
                }
            } else {
                initialsView
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
    }

    private var initialsView: some View {
        Circle()
            .fill(Color.accentColor.opacity(0.2))
            .overlay {
                if let initial {
                    Text(String(initial))
                        .font(.system(size: size * 0.4, weight: .semibold, design: .rounded))
                        .foregroundStyle(Color.accentColor)
                } else {
                    Image(systemName: "person.fill")
                        .font(.system(size: size * 0.45))
                        .foregroundStyle(Color.accentColor.opacity(0.6))
                }
            }
    }
}
