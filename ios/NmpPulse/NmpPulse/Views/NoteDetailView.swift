import SwiftUI

/// Screen 3 — one note expanded with its reply thread + a like button.
///
/// On appear it dispatches `nmp_app_open_thread(root_event_id)` (the kernel
/// compiles a REQ for the event + `#e` replies). The heart button dispatches
/// `nmp_app_react`; the reply button presents `ComposeView` with a
/// `replyToID`. The thread itself is read from `model.threadView` — a pure
/// mirror of the kernel's thread projection (D1: render whatever is there,
/// refine in place; no "if missing { spinner }" gate).
struct NoteDetailView: View {
    @EnvironmentObject private var model: KernelModel

    let rootItem: TimelineItem

    @State private var showReply = false

    var body: some View {
        List {
            Section {
                NoteRow(item: rootItem)
                HStack(spacing: 24) {
                    Button {
                        model.react(targetEventID: rootItem.id, reaction: "❤")
                    } label: {
                        Label("Like", systemImage: "heart")
                    }
                    Button {
                        showReply = true
                    } label: {
                        Label("Reply", systemImage: "arrowshape.turn.up.left")
                    }
                }
                .buttonStyle(.bordered)
                .font(.callout)
                .padding(.vertical, 4)
            }

            Section("Replies") {
                let replies = threadReplies
                if replies.isEmpty {
                    Text("No replies yet")
                        .foregroundStyle(.secondary)
                }
                ForEach(replies) { reply in
                    NoteRow(item: reply)
                }
            }
        }
        .listStyle(.plain)
        .navigationTitle("Thread")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            model.openThread(eventID: rootItem.id)
        }
        .sheet(isPresented: $showReply) {
            ComposeView(replyToID: rootItem.id)
        }
    }

    /// Reply set from the kernel's thread projection, excluding the root.
    private var threadReplies: [TimelineItem] {
        guard let thread = model.threadView,
              thread.rootEventId == rootItem.id || thread.focusedEventId == rootItem.id
        else {
            return []
        }
        return thread.items.filter { $0.id != rootItem.id }
    }
}
