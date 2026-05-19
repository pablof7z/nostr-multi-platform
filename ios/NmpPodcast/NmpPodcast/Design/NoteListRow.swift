import SwiftUI

// MARK: - NoteListRow

/// Reusable row for displaying a single `Note` inside a `List`.
///
/// Encapsulates the body layout (text + short date), trailing destructive
/// swipe-to-delete, leading swipe-to-edit, and context menu — the same
/// pattern previously duplicated in `ItemDetailSheet+Sections.swift` and
/// `FriendDetailView.swift`.
struct NoteListRow: View {
    let note: Note
    let onEdit: () -> Void
    let onDelete: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(note.text)
                .font(AppTheme.Typography.callout)
                .lineLimit(4)
            Text(note.createdAt.shortDate)
                .font(AppTheme.Typography.caption)
                .foregroundStyle(.tertiary)
        }
        .swipeActions(edge: .trailing, allowsFullSwipe: true) {
            Button(role: .destructive) {
                onDelete()
            } label: {
                Label("Delete", systemImage: "trash")
            }
        }
        .swipeActions(edge: .leading, allowsFullSwipe: false) {
            Button {
                onEdit()
            } label: {
                Label("Edit", systemImage: "pencil")
            }
            .tint(.blue)
        }
        .contextMenu {
            Button {
                onEdit()
            } label: {
                Label("Edit", systemImage: "pencil")
            }
            Button(role: .destructive) {
                onDelete()
            } label: {
                Label("Delete", systemImage: "trash")
            }
        }
    }
}

// MARK: - NotesSectionHeader

/// Reusable `Section` header with a title on the left and a plain "Add"
/// icon button on the right — used wherever notes lists appear in a `Form`
/// or `List`.
struct NotesSectionHeader: View {
    let title: String
    var count: Int = 0
    let onAdd: () -> Void

    var body: some View {
        HStack {
            Text(title)
            if count > 0 {
                Text("\(count)")
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button(action: onAdd) {
                Label("Add note", systemImage: "plus")
                    .labelStyle(.iconOnly)
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
        .textCase(nil)
    }
}
