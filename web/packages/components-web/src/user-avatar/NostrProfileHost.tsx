import { createContext, useContext, type JSX } from "solid-js";
import type { ProfileWire } from "./ProfileWire";

// Host bridge for the user-* components. The host app (gallery, Chirp, …) owns
// the kernel: it resolves `refs.profile` on `resolveProfileRef`, releases
// interest on `releaseProfileRef`, and exposes the resolved `ProfileWire`
// reactively through `profile`. Components never fetch or persist — they resolve
// on mount, release on cleanup, and render whatever `profile(pubkey)` returns.
//
// This mirrors the SwiftUI `NostrProfileHost` environment and the Compose
// `NostrProfileHost` interface so the contract is identical across platforms.
export interface NostrProfileHost {
  /** Reactive accessor — returns the resolved profile for a pubkey, or
   *  `undefined` until the kernel has ingested a kind:0 for it. Must be called
   *  inside a tracking scope (component body / JSX) to update on resolution. */
  profile(pubkey: string): ProfileWire | undefined;
  /** Register interest in a pubkey's profile. The kernel fetches kind:0 on the
   *  first ref resolve and refcounts by `consumerId`. */
  resolveProfileRef(pubkey: string, consumerId: string): void;
  /** Drop interest. The kernel can garbage-collect the subscription once every
   *  consumer releases. */
  releaseProfileRef(pubkey: string, consumerId: string): void;
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
  const host = useOptionalNostrProfileHost();
  if (!host) {
    throw new Error(
      "NostrProfileHost is missing — wrap your tree in <NostrProfileHostProvider host={...}>.",
    );
  }
  return host;
}

/** Read the ambient profile host when graceful fallback is valid. Content event
 *  refs use this so missing app-level host wiring degrades to raw links instead
 *  of crashing previews/tests. */
export function useOptionalNostrProfileHost(): NostrProfileHost | undefined {
  return useContext(NostrProfileHostContext);
}
