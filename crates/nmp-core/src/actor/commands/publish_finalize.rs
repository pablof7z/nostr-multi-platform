use crate::kernel::Kernel;
use nmp_signer_iface::UnsignedEvent;

fn stamp_unsigned_if_needed(kernel: &Kernel, unsigned: &mut UnsignedEvent) {
    if unsigned.created_at == 0 {
        unsigned.created_at = kernel.now_secs();
    }
}

pub(super) fn finalize_before_sign(kernel: &Kernel, unsigned: &mut UnsignedEvent) {
    stamp_unsigned_if_needed(kernel, unsigned);
    crate::publish::finalize_outbound_tags(unsigned.kind, &mut unsigned.tags, kernel);
}
