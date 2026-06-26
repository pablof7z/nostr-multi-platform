# ADR-0059 — Account lifecycle is separate from bootstrap publish

- **Status:** Accepted pending implementation
- **Date:** 2026-06-19
- **Doctrine:** `doctrine:d0` (app policy outside framework core),
  `doctrine:d4` (single writer), `doctrine:d6` (stateful failures),
  `doctrine:d7` (native executes capabilities only), `doctrine:d13`
  (secret-bearing boundaries)
- **Issues:** #1556, #1564
- **Related:** `docs/architecture/crate-boundaries.md` §4.2,
  `docs/builder-guide/11-sessions-signers.md`,
  `docs/ffi-surface.md` §5.

## 1. Problem

The current public account surface has two useful but incomplete paths:

1. `nmp_app_signin_nsec(app, secret, make_active)` imports a caller-supplied
   secret and does not publish profile, contacts, or relay-list metadata. It
   cannot generate a new visible account key inside Rust, and keyring
   persistence is owned by host/Marmot wrappers outside the generic lifecycle
   operation.
2. `nmp_app_create_new_account(app, profile_json, relays_json, mls,
   make_active)` generates a fresh local key, but `CreateAccount` immediately
   composes cold-start bootstrap publication: kind:0 profile, kind:10002 relay
   list, and optional kind:3 contacts when app Rust supplies seed follows.

That coupling makes "create or import an account, persist it, optionally make it
active, and do not publish anything yet" impossible as a generic NMP operation.
Apps can sometimes avoid publish by importing a secret they generated outside
the framework, but that moves account/key policy to the app boundary and loses
the single Rust-owned lifecycle.

Highlighter exposed the gap, but the rule is framework-wide: account lifecycle
and Nostr bootstrap publication are different operations. App onboarding order,
default follows, default relays, copy, and migration choreography are app-owned
Rust policy. NMP owns only the reusable account/key lifecycle and the explicit
bootstrap publish primitive.

## 2. Existing seams and why they are insufficient

- `ActorCommand::CreateAccount` generates a key and registers it, then proceeds
  into cold-start publish composition. Empty relay/profile/follow inputs reduce
  what is emitted, but the command still means "create account and run bootstrap
  side effects".
- `ActorCommand::AddSigner { source: LocalNsec, make_active }` is the right
  import primitive for caller-owned secrets. It does not cover Rust-generated
  visible accounts with keyring persistence before activation.
- `SignerSource::AppManagedLocalNsec` is intentionally hidden from account
  projections and rejected by active-account switching. It is for app-local
  automation keys, not human accounts.
- Marmot keyring helpers persist caller-provided secrets before calling
  `NmpApp::add_signer`, but they are host-wrapper composition, not the generic
  lifecycle contract.
- `publish_profile`, `publish_relay_list`, and app/protocol actions already
  show the desired shape for later bootstrap publication: explicit Rust-owned
  commands, not implicit side effects of key creation.

The missing seam is a generic lifecycle command that can create/import, persist,
activate, and publish bootstrap independently.

## 3. Decision

Introduce an additive versioned account-lifecycle ABI. Do not change the
signature or semantics of `nmp_app_create_new_account` in place.

```c
char *nmp_app_account_lifecycle_v1(
    NmpApp *app,
    const char *request_json,
    const char *secret_or_null
);
```

`request_json` is a versioned, NMP-owned schema. It is not opaque app-policy
JSON. Leaf app Rust crates may build the request, but native shells only pass it
through and execute capability callbacks. `secret_or_null` is used only by
secret-bearing operations such as local import and is wrapped in `Zeroizing` at
the FFI boundary before dispatch; raw secrets must not enter action history,
snapshots, logs, or generic JSON payloads.

The symbol returns the same enqueue verdict shape as
`nmp_app_dispatch_action_bytes`: `{"correlation_id":"..."}` or
`{"error":"..."}`. Terminal outcomes surface later in bounded state
(`account_lifecycle_stages`, `last_error_toast`, and account snapshots), never
as exceptions or blocking return values.

The v1 request owns four operations:

```text
CreateLocal {
  persist: None | KeyringRequired { account_id },
  make_active: bool,
  bootstrap: None | BootstrapBundle
}

ImportLocal {
  persist: None | KeyringRequired { account_id },
  make_active: bool,
  bootstrap: None | BootstrapBundle
}

Activate {
  account_id
}

PublishBootstrap {
  account_id: Active | Pubkey(String),
  bootstrap: BootstrapBundle
}
```

`BootstrapBundle` is explicit and optional. Absence means no Nostr publish, no
contact prepopulation, no relay-list publication, and no MLS key-package
autopublish. When present, it may contain:

- profile metadata for kind:0;
- relay rows for local configuration and/or kind:10002 publication;
- app-supplied contact pubkeys for kind:3;
- an MLS key-package publish intent.

Default relays, default follows, account names, onboarding copy, and migration
order never live in `nmp-core` or native shells. Leaf app Rust crates may wrap
the generic v1 request to inject their own policy, as Chirp already does for
seed follows.

## 4. Actor and persistence semantics

Account lifecycle is actor-owned. Native keychain/secure-storage code is a
capability executor only:

1. Rust creates or parses the local key material.
2. If `KeyringRequired` is present, Rust issues a typed keyring capability
   request with the caller-supplied `account_id`.
3. Native stores the raw secret and returns raw success/failure.
4. Rust decides whether the lifecycle operation succeeds, whether the account is
   projected, whether it becomes active, and whether bootstrap publish starts.

For `KeyringRequired`, successful persistence gates activation and bootstrap
publication. If persistence fails, the pending secret is zeroized, no account is
projected as ready, no account is made active, and no bootstrap publish occurs.
The failure is recorded under the lifecycle correlation id and may also surface
as a toast. This is D6: no panic, no silent partial success, no native retry
policy.

`persist: None` is allowed for tests, ephemeral local tooling, or hosts that own
secure persistence outside this NMP capability. That choice must be explicit in
the request.

## 5. Bootstrap publish semantics

Bootstrap publication is not part of account creation. It is an explicit
operation that can run immediately after create/import or later.

The implementation may reuse today's cold-start publish helpers, but the owner
changes:

- `CreateLocal` and `ImportLocal` register accounts and optionally activate them.
  They do not publish unless their request includes `bootstrap`.
- `PublishBootstrap` signs and publishes the selected bundle for the selected
  account. Cold-start routing may use declared relay rows plus discovery relays,
  but relay choice remains Rust-owned.
- Empty bundle fields are no-ops. An empty contacts list publishes no kind:3.
- App-supplied follows remain app policy. `nmp-core` never reintroduces default
  follows.
- MLS key-package autopublish is explicit bundle content. It is no longer an
  implicit side effect of "active local-key sign-in" for this lifecycle path.

The legacy `nmp_app_create_new_account` and app wrappers must become
compatibility adapters over the new single implementation path. They may keep
today's observable behavior by passing an explicit `BootstrapBundle`, but they
must not preserve a second core code path for "create plus bootstrap".

## 6. ABI and documentation migration

The implementation PR must:

- add `nmp_app_account_lifecycle_v1` without removing existing symbols;
- keep `nmp_app_create_new_account` source- and binary-compatible;
- update generated headers, Swift/Kotlin/TUI call sites only where they adopt
  the new symbol;
- update `docs/ffi-surface.md` once the symbol exists;
- update `docs/builder-guide/11-sessions-signers.md` with lifecycle guidance;
- add scoped tests proving create-without-bootstrap emits no publish outbound,
  required keyring failure does not activate or publish, and legacy create still
  preserves its old bootstrap behavior through the shared path.

If the implementation changes any public symbol, moves command variants, or
renames public types, it must run the touched-crate tests, doctrine lint, and
`cargo build --workspace`.

## 7. Non-goals

- No Highlighter-specific defaults or onboarding choreography in NMP crates.
- No native shell sequencing beyond passing requests and executing raw
  capabilities.
- No opaque app-policy JSON tunnel through generic FFI.
- No polling for keyring completion or publish completion.
- No second account representation for "created but not published"; lifecycle
  stage state is transient operation state, while the account snapshot remains
  the durable visible account projection.
