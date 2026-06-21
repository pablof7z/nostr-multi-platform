import { createContext, useContext, type JSX } from "solid-js";
import type { ProfileWire } from "./ProfileWire";

// Host bridge for the user-* components. The host app (gallery, Chirp, …) owns
// the kernel: it fetches kind:0 on `claimProfile`, releases interest on
// `releaseProfile`, and exposes the resolved `ProfileWire` reactively through
// `profile`. Components never fetch or persist — they claim on mount, release
// on cleanup, and render whatever `profile(pubkey)` currently returns.
//
// This mirrors the SwiftUI `NostrProfileHost` environment and the Compose
// `NostrProfileHost` interface so the contract is identical across platforms.
export interface NostrProfileHost {
  /** Reactive accessor — returns the resolved profile for a pubkey, or
   *  `undefined` until the kernel has ingested a kind:0 for it. Must be called
   *  inside a tracking scope (component body / JSX) to update on resolution. */
  profile(pubkey: string): ProfileWire | undefined;
  /** Register interest in a pubkey's profile. The kernel fetches kind:0 on the
   *  first claim and refcounts by `consumerId`. */
  claimProfile(pubkey: string, consumerId: string): void;
  /** Drop interest. The kernel can garbage-collect the subscription once every
   *  consumer releases. */
  releaseProfile(pubkey: string, consumerId: string): void;
}

const NostrProfileHostContext = createContext<NostrProfileHost>();

export function NostrProfileHostProvider(props: {
  host: NostrProfileHost;
  children: JSX.Element;
}): JSX.Element {
  return (
    <NostrProfileHostContext.Provider value={props.host}>
      {props.children}
    </NostrProfileHostContext.Provider>
  );
}

/** Read the ambient profile host. Throws if no provider is mounted — every
 *  user-* component requires a host so a missing one is a wiring bug, not a
 *  silent degraded render. */
export function useNostrProfileHost(): NostrProfileHost {
  const host = useContext(NostrProfileHostContext);
  if (!host) {
    throw new Error(
      "NostrProfileHost is missing — wrap your tree in <NostrProfileHostProvider host={...}>.",
    );
  }
  return host;
}
