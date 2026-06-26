// NmpClientContext — SolidJS context for the shell's NmpClient + snapshot.
//
// Item B exposes this context so Item C (feed/profile UI) and Item D
// (signing/onboarding) can subscribe to the running runtime without modifying
// App.tsx. Components call `useNmpClient()` or `useSnapshot()` to access the
// client and live snapshot signal.
//
// Registration pattern: Item C registers feed/profile panel components by
// importing and rendering them inside the context tree; Item D registers the
// signing panel the same way. App.tsx never needs to know their internals —
// the context is the shared seam.

import {
  createContext,
  useContext,
  type Accessor,
  type JSX,
} from "solid-js";
import type { NmpClient, RuntimeSnapshot } from "./client";

/** Value provided by `NmpClientProvider`. */
export type NmpClientContextValue = {
  /** The running NmpClient (worker or in-process fallback). */
  client: NmpClient;
  /** Reactive accessor for the current snapshot. */
  snapshot: Accessor<RuntimeSnapshot>;
};

/** Context created once in the composition root (App.tsx) and consumed by
 *  Item C and Item D without any coupling back to the root. */
export const NmpClientContext = createContext<NmpClientContextValue>();

/** Provider — wraps the app tree so all descendants can call `useNmpClient()`.
 *  Created once per page load in App.tsx. */
export function NmpClientProvider(props: {
  client: NmpClient;
  snapshot: Accessor<RuntimeSnapshot>;
  children: JSX.Element;
}): JSX.Element {
  return (
    <NmpClientContext.Provider value={{ client: props.client, snapshot: props.snapshot }}>
      {props.children}
    </NmpClientContext.Provider>
  );
}

/** Access the running `NmpClient` + reactive snapshot signal.
 *
 * Throws if called outside `NmpClientProvider` — always mount inside App. */
export function useNmpClient(): NmpClientContextValue {
  const ctx = useContext(NmpClientContext);
  if (!ctx) {
    throw new Error(
      "[chirp] useNmpClient must be called inside NmpClientProvider. " +
        "Ensure the component is rendered inside App's context tree.",
    );
  }
  return ctx;
}

/** Shorthand: reactive accessor for the current snapshot.
 *
 * ```tsx
 * const snapshot = useSnapshot();
 * const status = () => snapshot().status;
 * ```
 *
 * Item C uses this to read `latestUpdateBytes` and decode FlatBuffers
 * projections. Item D uses it to read `events` and detect pending sign
 * requests. */
export function useSnapshot(): Accessor<RuntimeSnapshot> {
  return useNmpClient().snapshot;
}
