//! TypeScript scalar field emission helpers for flat-table action builders.

use crate::action_builders::registry::{FieldKind, PayloadField};

pub(crate) fn render_scalar_field(field: &PayloadField, slot: usize, out: &mut String) {
    match field.kind {
        FieldKind::Uint => render_int(field, slot, "fbb.addFieldInt32", out),
        FieldKind::Ulong => render_bigint(field, slot, out),
        FieldKind::UlongWithPresenceFlag { flag_name } => {
            let slot_flag = slot + 1;
            out.push_str(&format!(
                "    if ({n} !== null) {{\n\
                 \x20     fbb.addFieldInt64({slot}, {n}, BigInt(0)); // slot {slot}: {n}\n\
                 \x20     fbb.addFieldInt8({slot_flag}, 1, 0); // slot {slot_flag}: {flag} (bool)\n\
                 \x20   }}\n",
                n = field.name,
                slot = slot,
                slot_flag = slot_flag,
                flag = flag_name,
            ));
        }
        FieldKind::Ubyte | FieldKind::Sbyte => render_int(field, slot, "fbb.addFieldInt8", out),
        _ => unreachable!("non-scalar action-builder field"),
    }
}

fn render_int(field: &PayloadField, slot: usize, add_fn: &str, out: &mut String) {
    if field.optional {
        out.push_str(&format!(
            "    if ({n} !== null) {add_fn}({slot}, {n}, 0); // slot {slot}: {n}\n",
            n = field.name,
            add_fn = add_fn
        ));
    } else {
        out.push_str(&format!(
            "    {add_fn}({slot}, {n}, 0); // slot {slot}: {n}\n",
            n = field.name,
            add_fn = add_fn
        ));
    }
}

fn render_bigint(field: &PayloadField, slot: usize, out: &mut String) {
    if field.optional {
        out.push_str(&format!(
            "    if ({n} !== null) fbb.addFieldInt64({slot}, {n}, BigInt(0)); // slot {slot}: {n}\n",
            n = field.name
        ));
    } else {
        out.push_str(&format!(
            "    fbb.addFieldInt64({slot}, {n}, BigInt(0)); // slot {slot}: {n}\n",
            n = field.name
        ));
    }
}
