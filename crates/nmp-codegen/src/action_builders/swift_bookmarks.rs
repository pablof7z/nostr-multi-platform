//! ADR-0064 §3 (#1783 / #2197) — Swift emitter for the NIP-51 bookmark builders.
//!
//! Split out of [`crate::action_builders::swift`] purely as a size-management
//! seam (AGENTS.md / V-12): the flat-table emitter plus these nested-table
//! bookmark emitters together would exceed the 500-LOC ceiling. The bookmark
//! list (`nmp.nip51.add_bookmark` / `remove_bookmark`) and bookmark-set
//! (`nmp.nip51.add_bookmark_set_item` / `remove_bookmark_set_item`) namespaces
//! carry nested-table payloads (`BookmarkItem` inside the update payload), so
//! they are hand-rolled rather than driven by the generic flat-table
//! [`crate::action_builders::registry::PayloadField`] list.

use crate::action_builders::registry::ActionBuilder;
use crate::action_contract::contract_for;

/// True for the kind:10003 bookmark-list add/remove namespaces.
pub(crate) fn is_bookmark_builder(builder: &ActionBuilder) -> bool {
    matches!(
        builder.namespace,
        "nmp.nip51.add_bookmark" | "nmp.nip51.remove_bookmark"
    )
}

/// True for the kind:30003/30004 bookmark-set item add/remove namespaces.
pub(crate) fn is_bookmark_set_builder(builder: &ActionBuilder) -> bool {
    matches!(
        builder.namespace,
        "nmp.nip51.add_bookmark_set_item" | "nmp.nip51.remove_bookmark_set_item"
    )
}

/// Render one bookmark-list add/remove builder (nested `BookmarkItem` table).
pub(crate) fn render_bookmark_update(builder: &ActionBuilder, out: &mut String) {
    let contract = contract_for(builder.namespace);
    let method = builder.method;
    out.push_str(&format!("    /// {}\n", builder.doc));
    out.push_str(&format!(
        "    /// Builds the `{}` `DispatchEnvelope` bytes for the byte doorway.\n",
        builder.namespace
    ));
    out.push_str(&format!(
        "    public static func {method}(\n\
         \x20       correlationId: String,\n\
         \x20       accountPubkey: String,\n\
         \x20       itemKind: UInt8,\n\
         \x20       value: String,\n\
         \x20       relay: String?\n\
         \x20   ) -> [UInt8] {{\n"
    ));
    out.push_str("        var fbb = FlatBufferBuilder()\n");
    out.push_str("        let accountPubkeyOffset = fbb.create(string: accountPubkey)\n");
    out.push_str("        let valueOffset = fbb.create(string: value)\n");
    out.push_str(
        "        let relayOffset: Offset = relay.map { fbb.create(string: $0) } ?? Offset()\n",
    );
    out.push_str("        let itemStart = fbb.startTable(with: 3)\n");
    out.push_str("        fbb.add(element: itemKind, def: UInt8(0), at: 4) // slot 0: kind\n");
    out.push_str("        fbb.add(offset: valueOffset, at: 6) // slot 1: value\n");
    out.push_str(
        "        if relayOffset.o != 0 { fbb.add(offset: relayOffset, at: 8) } // slot 2: relay\n",
    );
    out.push_str("        let itemRoot = Offset(offset: fbb.endTable(at: itemStart))\n");
    out.push_str("        let payloadStart = fbb.startTable(with: 3)\n");
    out.push_str(&format!(
        "        fbb.add(element: UInt32({}), def: UInt32(0), at: 4) // slot 0: schema_version\n",
        contract.schema_version
    ));
    out.push_str("        fbb.add(offset: accountPubkeyOffset, at: 6) // slot 1: account_pubkey\n");
    out.push_str("        fbb.add(offset: itemRoot, at: 8) // slot 2: item\n");
    out.push_str("        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))\n");
    out.push_str(&format!(
        "        fbb.finish(offset: payloadRoot, fileId: {:?})\n",
        contract.file_identifier
    ));
    out.push_str("        let payload = fbb.sizedByteArray\n");
    out.push_str(&format!(
        "        return encodeDispatchEnvelope(\n\
         \x20           correlationId: correlationId,\n\
         \x20           actionNamespace: {:?},\n\
         \x20           payload: payload\n\
         \x20       )\n",
        builder.namespace
    ));
    out.push_str("    }\n");
}

/// Render one bookmark-set item add/remove builder (nested `BookmarkItem`
/// table inside a `BookmarkSetUpdatePayload` carrying set_kind + identifier).
pub(crate) fn render_bookmark_set_update(builder: &ActionBuilder, out: &mut String) {
    let contract = contract_for(builder.namespace);
    let method = builder.method;
    out.push_str(&format!("    /// {}\n", builder.doc));
    out.push_str(&format!(
        "    /// Builds the `{}` `DispatchEnvelope` bytes for the byte doorway.\n",
        builder.namespace
    ));
    out.push_str(&format!(
        "    public static func {method}(\n\
         \x20       correlationId: String,\n\
         \x20       accountPubkey: String,\n\
         \x20       setKind: UInt8,\n\
         \x20       identifier: String,\n\
         \x20       itemKind: UInt8,\n\
         \x20       value: String,\n\
         \x20       relay: String?\n\
         \x20   ) -> [UInt8] {{\n"
    ));
    out.push_str("        var fbb = FlatBufferBuilder()\n");
    out.push_str("        let accountPubkeyOffset = fbb.create(string: accountPubkey)\n");
    out.push_str("        let identifierOffset = fbb.create(string: identifier)\n");
    out.push_str("        let valueOffset = fbb.create(string: value)\n");
    out.push_str(
        "        let relayOffset: Offset = relay.map { fbb.create(string: $0) } ?? Offset()\n",
    );
    // Build nested BookmarkItem table (3 slots: kind ubyte, value string, relay string)
    out.push_str("        let itemStart = fbb.startTable(with: 3)\n");
    out.push_str("        fbb.add(element: itemKind, def: UInt8(0), at: 4) // slot 0: kind\n");
    out.push_str("        fbb.add(offset: valueOffset, at: 6) // slot 1: value\n");
    out.push_str(
        "        if relayOffset.o != 0 { fbb.add(offset: relayOffset, at: 8) } // slot 2: relay\n",
    );
    out.push_str("        let itemRoot = Offset(offset: fbb.endTable(at: itemStart))\n");
    // Build BookmarkSetUpdatePayload root table (5 slots: schema_version, account_pubkey, set_kind, identifier, item)
    out.push_str("        let payloadStart = fbb.startTable(with: 5)\n");
    out.push_str(&format!(
        "        fbb.add(element: UInt32({}), def: UInt32(0), at: 4) // slot 0: schema_version\n",
        contract.schema_version
    ));
    out.push_str("        fbb.add(offset: accountPubkeyOffset, at: 6) // slot 1: account_pubkey\n");
    out.push_str("        fbb.add(element: setKind, def: UInt8(0), at: 8) // slot 2: set_kind\n");
    out.push_str("        fbb.add(offset: identifierOffset, at: 10) // slot 3: identifier\n");
    out.push_str("        fbb.add(offset: itemRoot, at: 12) // slot 4: item\n");
    out.push_str("        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))\n");
    out.push_str(&format!(
        "        fbb.finish(offset: payloadRoot, fileId: {:?})\n",
        contract.file_identifier
    ));
    out.push_str("        let payload = fbb.sizedByteArray\n");
    out.push_str(&format!(
        "        return encodeDispatchEnvelope(\n\
         \x20           correlationId: correlationId,\n\
         \x20           actionNamespace: {:?},\n\
         \x20           payload: payload\n\
         \x20       )\n",
        builder.namespace
    ));
    out.push_str("    }\n");
}
