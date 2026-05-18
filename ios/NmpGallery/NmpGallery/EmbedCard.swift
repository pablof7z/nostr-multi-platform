import SwiftUI

/// Embedded event card (quoted note / nevent / naddr). Applies the Swift
/// `RenderContext` mirror: collapses at depth >= 4 (PD-015) or on a
/// `visited`-set cycle, and degrades gracefully for dangling / unsupported
/// targets (D1 — never blank, never a spinner).
struct EmbedCard: View {
    let uri: String
    let refID: String
    let entry: EmbedEntry?
    let embeds: [String: EmbedEntry]
    let ctx: RenderContext

    var body: some View {
        guard let entry else {
            return AnyView(stub("Quoted event unavailable",
                                detail: refID, system: "questionmark.app"))
        }

        // Bundle-time context-independent collapse facts.
        if entry.collapsed {
            switch entry.collapseReason {
            case "dangling":
                return AnyView(stub("Quoted event unavailable",
                                    detail: refID,
                                    system: "wifi.slash"))
            case "unsupported":
                return AnyView(stub(
                    "Unsupported event (kind \(entry.resolvedKind))",
                    detail: refID, system: "shippingbox"))
            default:
                return AnyView(stub("Embed collapsed",
                                    detail: refID, system: "rectangle.slash"))
            }
        }

        // Render-time PD-015 depth + cycle guard (Swift RenderContext).
        if let ev = entry.event {
            let key = visitedKey(for: ev)
            let (collapse, reason) = ctx.shouldCollapse(into: key)
            if collapse {
                let label = reason == "cycle"
                    ? "Already shown (cycle broken)"
                    : "Quoted event (tap to open)"
                return AnyView(stub(label, detail: refID,
                                    system: reason == "cycle"
                                        ? "arrow.triangle.2.circlepath"
                                        : "chevron.right.circle"))
            }
        }

        return AnyView(card(entry))
    }

    @ViewBuilder
    private func card(_ entry: EmbedEntry) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if let article = entry.article {
                ArticlePreview(header: article)
            } else if let list = entry.list {
                ListCard(list: list)
            }
            if let ev = entry.event {
                HStack(spacing: 6) {
                    Identicon(seed: ev.pubkey)
                        .frame(width: 18, height: 18)
                    Text("kind \(ev.kind) · @npub1\(ev.pubkey.prefix(6))…")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            if let body = entry.rendered, entry.article == nil,
               entry.list == nil {
                SegmentDtoView(
                    tree: body, embeds: embeds,
                    ctx: childCtx(entry))
            }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color.secondary.opacity(0.4), lineWidth: 1))
        .background(Color(.tertiarySystemBackground))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    private func childCtx(_ entry: EmbedEntry) -> RenderContext {
        guard let ev = entry.event else { return ctx }
        return ctx.descend(into: visitedKey(for: ev))
    }

    @ViewBuilder
    private func stub(_ title: String, detail: String,
                      system: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: system)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.caption.bold())
                Text(detail.prefix(28) + "…")
                    .font(.caption2.monospaced())
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.secondary.opacity(0.10))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }
}

/// Medium-like article preview card (naddr → kind:30023).
struct ArticlePreview: View {
    let header: ArticleHeaderDto

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Label("Article", systemImage: "doc.richtext")
                .font(.caption2.bold())
                .foregroundStyle(.teal)
            Text(header.title ?? "(untitled)")
                .font(.headline)
            if let s = header.summary {
                Text(s).font(.caption).foregroundStyle(.secondary)
            }
            HStack(spacing: 6) {
                Identicon(seed: header.author)
                    .frame(width: 16, height: 16)
                Text("@npub1\(header.author.prefix(6))…")
                    .font(.caption2).foregroundStyle(.secondary)
                Spacer()
                Text("Read article ›")
                    .font(.caption2.bold())
                    .foregroundStyle(.teal)
            }
        }
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.teal.opacity(0.10))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }
}

/// NIP-51 inline titled list card (follow set / bookmarks / relay list).
struct ListCard: View {
    let list: ListDto

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Label(list.title ?? "List", systemImage: "list.bullet")
                .font(.caption.bold())
                .foregroundStyle(.green)
            if list.rows.isEmpty {
                Text("(no members)")
                    .font(.caption2).italic()
                    .foregroundStyle(.tertiary)
            }
            ForEach(Array(list.rows.enumerated()), id: \.offset) {
                _, row in
                rowView(row)
            }
        }
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.green.opacity(0.10))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    @ViewBuilder
    private func rowView(_ row: ListRowDto) -> some View {
        switch row {
        case let .profile(pubkey, name, _):
            HStack(spacing: 6) {
                Identicon(seed: pubkey).frame(width: 16, height: 16)
                Text(name.map { "@\($0)" }
                    ?? "@npub1\(pubkey.prefix(6))…")
                    .font(.caption)
            }
        case let .event(id):
            Label("note \(id.prefix(10))…", systemImage: "text.quote")
                .font(.caption2)
        case let .address(coord):
            Label(coord, systemImage: "doc.text")
                .font(.caption2).lineLimit(1)
        case let .hashtag(tag):
            Label("#\(tag)", systemImage: "number")
                .font(.caption2)
        case let .relay(url, read, write):
            HStack(spacing: 4) {
                Image(systemName: "antenna.radiowaves.left.and.right")
                Text(url).font(.caption2.monospaced())
                if read { tagBadge("R") }
                if write { tagBadge("W") }
            }
        case let .unknown(t):
            Text("[\(t)]").font(.caption2).foregroundStyle(.red)
        }
    }

    @ViewBuilder
    private func tagBadge(_ s: String) -> some View {
        Text(s)
            .font(.caption2.bold())
            .padding(.horizontal, 4)
            .background(Color.green.opacity(0.25))
            .clipShape(Capsule())
    }
}
