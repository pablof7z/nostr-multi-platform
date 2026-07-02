//! Kotlin scalar field emission helpers for flat-table action builders.

use crate::action_builders::registry::{FieldKind, PayloadField};

pub(crate) fn render_scalar_field(field: &PayloadField, slot: usize, out: &mut String) {
    match field.kind {
        FieldKind::Uint => render_int(field, slot, out),
        FieldKind::Ulong => render_long(field, slot, out),
        FieldKind::UlongWithPresenceFlag { flag_name } => {
            let slot_flag = slot + 1;
            out.push_str(&format!(
                "        if ({n} != null) {{\n\
                 \x20           fbb.addLong({slot}, {n}, 0L) // slot {slot}: {n}\n\
                 \x20           fbb.addBoolean({slot_flag}, true, false) // slot {slot_flag}: {flag}\n\
                 \x20       }}\n",
                n = field.name,
                slot = slot,
                slot_flag = slot_flag,
                flag = flag_name,
            ));
        }
        FieldKind::Ubyte | FieldKind::Sbyte => render_byte(field, slot, out),
        _ => unreachable!("non-scalar action-builder field"),
    }
}

fn render_int(field: &PayloadField, slot: usize, out: &mut String) {
    if field.optional {
        out.push_str(&format!(
            "        if ({n} != null) fbb.addInt({slot}, {n}, 0) // slot {slot}: {n}\n",
            n = field.name
        ));
    } else {
        out.push_str(&format!(
            "        fbb.addInt({slot}, {n}, 0) // slot {slot}: {n}\n",
            n = field.name
        ));
    }
}

fn render_long(field: &PayloadField, slot: usize, out: &mut String) {
    if field.optional {
        out.push_str(&format!(
            "        if ({n} != null) fbb.addLong({slot}, {n}, 0L) // slot {slot}: {n}\n",
            n = field.name
        ));
    } else {
        out.push_str(&format!(
            "        fbb.addLong({slot}, {n}, 0L) // slot {slot}: {n}\n",
            n = field.name
        ));
    }
}

fn render_byte(field: &PayloadField, slot: usize, out: &mut String) {
    if field.optional {
        out.push_str(&format!(
            "        if ({n} != null) fbb.addByte({slot}, {n}, 0) // slot {slot}: {n}\n",
            n = field.name
        ));
    } else {
        out.push_str(&format!(
            "        fbb.addByte({slot}, {n}, 0) // slot {slot}: {n}\n",
            n = field.name
        ));
    }
}
