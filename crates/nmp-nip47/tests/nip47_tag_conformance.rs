//! NIP-47 NWC tag conformance — moved from
//! `crates/nmp-core/tests/nip_tag_conformance.rs` in V-38.
//!
//! The test drives `WalletConnectCommand` through the public
//! `ProtocolCommand` seam against a real test-support `Kernel`, then asserts
//! the emitted kind:23194 request events carry the wallet pubkey `p` tag.

use nmp_core::substrate::{
    NoopActionStageTracker, NoopErrorSurface, NoopHostOpHandlerAccess, NoopKernelClock,
    NoopLocalSignerAccess, NoopRecipientRelayLookup, NoopZapProfileLookup, ProtocolCommand,
    ProtocolCommandContext, ProtocolCommandContextParts, WalletKernelAccess,
};
use nmp_core::testing::{ActorCommand, Kernel, DEFAULT_VISIBLE_LIMIT};
use nmp_core::{CommandSender, OutboundMessage};
use nmp_nip47::{
    new_wallet_runtime_handle, new_wallet_status_slot, WalletConnectCommand, WalletRuntime,
};
use nostr::Keys;
use serde_json::Value;

fn make_nwc_uri(wallet: &Keys, client: &Keys) -> String {
    format!(
        "nostr+walletconnect://{}?relay=wss://wallet.example&secret={}",
        wallet.public_key().to_hex(),
        client.secret_key().to_secret_hex()
    )
}

fn ignore_actor_command(_cmd: ActorCommand) {}

fn context<'a>(
    wallet_kernel: &'a dyn WalletKernelAccess,
    command_sender: &'a CommandSender,
    outbound: &'a mut Vec<OutboundMessage>,
) -> ProtocolCommandContext<'a> {
    static CLOCK: NoopKernelClock = NoopKernelClock;
    static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
    static DMS: nmp_core::substrate::EmptyDmInboxRelayLookup =
        nmp_core::substrate::EmptyDmInboxRelayLookup;
    static ERRORS: NoopErrorSurface = NoopErrorSurface;
    static STAGES: NoopActionStageTracker = NoopActionStageTracker;
    static RECIPIENTS: NoopRecipientRelayLookup = NoopRecipientRelayLookup;
    static HOST_OP: NoopHostOpHandlerAccess = NoopHostOpHandlerAccess;
    static ZAP: NoopZapProfileLookup = NoopZapProfileLookup;
    ProtocolCommandContext::new(ProtocolCommandContextParts {
        send: &ignore_actor_command,
        command_sender: command_sender.clone(),
        clock: &CLOCK,
        signers: &SIGNERS,
        dms: &DMS,
        errors: &ERRORS,
        stages: &STAGES,
        recipients: &RECIPIENTS,
        host_op_handler: &HOST_OP,
        wallet_kernel,
        zap_profiles: &ZAP,
    })
    .with_outbound(outbound)
}

#[test]
fn kind23194_nwc_request_carries_wallet_p_tag() {
    let wallet_keys = Keys::generate();
    let client_keys = Keys::generate();
    let uri = make_nwc_uri(&wallet_keys, &client_keys);

    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let wallet_status = new_wallet_status_slot();
    let runtime = new_wallet_runtime_handle();
    *runtime.lock().expect("wallet runtime slot") = Some(WalletRuntime::new(wallet_status));
    let (command_sender, _rx) = CommandSender::bounded_channel();
    let mut outbound = Vec::new();

    let wallet_access = kernel.as_wallet_access();
    let mut ctx = context(&wallet_access, &command_sender, &mut outbound);
    Box::new(WalletConnectCommand { uri, runtime })
        .run(&mut ctx)
        .expect("wallet_connect command should run");

    let wallet_pubkey = wallet_keys.public_key().to_hex();
    let nwc_events: Vec<Value> = outbound
        .iter()
        .filter_map(|msg| serde_json::from_str::<Value>(msg.text()).ok())
        .filter(|frame| frame.get(0).and_then(Value::as_str) == Some("EVENT"))
        .filter_map(|frame| frame.get(1).cloned())
        .filter(|event| event.get("kind").and_then(Value::as_u64) == Some(23194))
        .collect();

    assert!(
        !nwc_events.is_empty(),
        "wallet_connect must emit at least one kind:23194 request"
    );
    for event in nwc_events {
        let tags = event
            .get("tags")
            .and_then(Value::as_array)
            .expect("kind:23194 event must carry tags");
        assert!(
            tags.iter().any(|tag| {
                tag.as_array().is_some_and(|columns| {
                    columns.first().and_then(Value::as_str) == Some("p")
                        && columns.get(1).and_then(Value::as_str) == Some(wallet_pubkey.as_str())
                })
            }),
            "kind:23194 event must carry wallet pubkey p tag: {event}"
        );
    }
}
