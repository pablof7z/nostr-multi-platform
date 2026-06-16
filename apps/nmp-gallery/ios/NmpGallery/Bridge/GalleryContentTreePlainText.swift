import Foundation

enum GalleryContentTreePlainText {
    static func decode(_ data: Data) -> String {
        guard data.count >= 8 else { return "" }
        let reader = GalleryFlatBufferReader(data: data)
        guard reader.hasIdentifier("NFCT"),
              let root = try? reader.rootTable() else {
            return ""
        }
        let roots = (try? reader.u32VectorField(table: root, index: 1)) ?? []
        return roots.compactMap { nodeText(index: $0, root: root, reader: reader) }
            .joined(separator: "")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func nodeText(index: UInt32, root: Int, reader: GalleryFlatBufferReader) -> String {
        guard let nodes = try? reader.tableVectorField(table: root, index: 0),
              Int(index) < nodes.count else {
            return ""
        }
        return nodeText(table: nodes[Int(index)], root: root, reader: reader)
    }

    private static func nodeText(table: Int, root: Int, reader: GalleryFlatBufferReader) -> String {
        let kind = (try? reader.u8Field(table: table, index: 0)) ?? 0
        switch kind {
        case 0:
            return (try? reader.stringField(table: table, index: 1)) ?? ""
        case 1, 2:
            guard let uri = try? reader.tableField(table: table, index: 5) else { return "" }
            return (try? reader.stringField(table: uri, index: 0)) ?? ""
        case 3:
            return "#" + ((try? reader.stringField(table: table, index: 3)) ?? "")
        case 4:
            return (try? reader.stringField(table: table, index: 2)) ?? ""
        case 8, 9, 10, 14, 15, 17:
            return childrenText(table: table, root: root, reader: reader)
        case 11:
            return (try? reader.stringField(table: table, index: 13)) ?? ""
        case 12:
            return listText(table: table, root: root, reader: reader)
        case 16:
            return (try? reader.stringField(table: table, index: 1)) ?? ""
        case 18:
            return (try? reader.stringField(table: table, index: 15)) ?? ""
        case 19:
            return " "
        case 20:
            return "\n"
        default:
            return ""
        }
    }

    private static func childrenText(table: Int, root: Int, reader: GalleryFlatBufferReader) -> String {
        let children = (try? reader.u32VectorField(table: table, index: 4)) ?? []
        return children.map { nodeText(index: $0, root: root, reader: reader) }.joined()
    }

    private static func listText(table: Int, root: Int, reader: GalleryFlatBufferReader) -> String {
        let items = (try? reader.tableVectorField(table: table, index: 11)) ?? []
        return items.map { item in
            let children = (try? reader.u32VectorField(table: item, index: 0)) ?? []
            return children.map { nodeText(index: $0, root: root, reader: reader) }.joined()
        }.joined(separator: "\n")
    }
}
