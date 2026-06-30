use crate::rpc::RpcBuildError;

/// Map a [`RpcBuildError`] from the shared frame builder onto the ACK-specific
/// error strings the pre-extraction inline code produced.
pub(super) fn map_ack_build_error(e: &RpcBuildError) -> String {
    match e {
        RpcBuildError::Encrypt(s) => format!("nip44 encrypt ack: {s}"),
        RpcBuildError::TagParse(s) => format!("tag parse: {s}"),
        RpcBuildError::Sign(s) => format!("sign ack event: {s}"),
        RpcBuildError::Serialize(s) => format!("serialize ack: {s}"),
    }
}
