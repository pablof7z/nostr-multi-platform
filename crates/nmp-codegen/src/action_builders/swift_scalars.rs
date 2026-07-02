//! Swift scalar field emission helpers for flat-table action builders.

use crate::action_builders::registry::{FieldKind, PayloadField};

pub(crate) fn render_scalar_field(
    field: &PayloadField,
    slot: usize,
    vtoffset: usize,
    out: &mut String,
) {
    match field.kind {
        FieldKind::Uint => render_uint(field, slot, vtoffset, out),
        FieldKind::Ulong => render_ulong(field, slot, vtoffset, out),
        FieldKind::UlongWithPresenceFlag { flag_name } => {
            let slot_flag = slot + 1;
            out.push_str(&format!(
                "        if let {n}Val = {n} {{\n\
                 \x20           fbb.add(element: {n}Val, def: UInt64(0), at: {vt}) // slot {slot}: {n}\n\
                 \x20           fbb.add(element: true, def: false, at: {vt_flag}) // slot {slot_flag}: {flag}\n\
                 \x20       }}\n",
                n = field.name,
                vt = vtoffset,
                vt_flag = vtoffset + 2,
                slot = slot,
                slot_flag = slot_flag,
                flag = flag_name,
            ));
        }
        FieldKind::Ubyte => render_byte(field, slot, vtoffset, "UInt8", out),
        FieldKind::Sbyte => render_byte(field, slot, vtoffset, "Int8", out),
        _ => unreachable!("non-scalar action-builder field"),
    }
}

fn render_uint(field: &PayloadField, slot: usize, vtoffset: usize, out: &mut String) {
    if field.optional {
        out.push_str(&format!(
            "        if let {n}Val = {n} {{ fbb.add(element: UInt32({n}Val), def: UInt32(0), at: {vt}) }} // slot {slot}: {n}\n",
            n = field.name,
            vt = vtoffset
        ));
    } else {
        out.push_str(&format!(
            "        fbb.add(element: UInt32({n}), def: UInt32(0), at: {vt}) // slot {slot}: {n}\n",
            n = field.name,
            vt = vtoffset
        ));
    }
}

fn render_ulong(field: &PayloadField, slot: usize, vtoffset: usize, out: &mut String) {
    if field.optional {
        out.push_str(&format!(
            "        if let {n}Val = {n} {{ fbb.add(element: {n}Val, def: UInt64(0), at: {vt}) }} // slot {slot}: {n}\n",
            n = field.name,
            vt = vtoffset
        ));
    } else {
        out.push_str(&format!(
            "        fbb.add(element: {n}, def: UInt64(0), at: {vt}) // slot {slot}: {n}\n",
            n = field.name,
            vt = vtoffset
        ));
    }
}

fn render_byte(field: &PayloadField, slot: usize, vtoffset: usize, ty: &str, out: &mut String) {
    if field.optional {
        out.push_str(&format!(
            "        if let {n}Val = {n} {{ fbb.add(element: {n}Val, def: {ty}(0), at: {vt}) }} // slot {slot}: {n}\n",
            n = field.name,
            vt = vtoffset,
            ty = ty
        ));
    } else {
        out.push_str(&format!(
            "        fbb.add(element: {n}, def: {ty}(0), at: {vt}) // slot {slot}: {n}\n",
            n = field.name,
            vt = vtoffset,
            ty = ty
        ));
    }
}
