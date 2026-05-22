import { describe, expect, it } from "vitest";
import {
  connectNip46,
  joinGroup,
  openBech32,
  openGroup,
  refreshTimeline,
  requestProfile,
  saveRelays,
  search,
  sendDirectMessage,
  sendGroupMessage,
  setFollow,
  startOnboarding,
  submitNote,
} from "./actions";
import type { NmpClient, RuntimeSnapshot } from "../nmp/client";

type DispatchCall = {
  actionType: string;
  payload: unknown;
};

function createDispatchClient() {
  const calls: DispatchCall[] = [];
  const snapshot: RuntimeSnapshot = { status: "ready", events: [] };
  const client: NmpClient = {
    snapshot: () => snapshot,
    subscribe: () => () => undefined,
    start: async () => snapshot,
    dispatch: async (actionType, payload) => {
      calls.push({ actionType, payload });
      return snapshot;
    },
  };
  return { calls, client };
}

describe("Chirp action dispatch helpers", () => {
  it("redacts imported nsec values before dispatching onboarding", async () => {
    const { calls, client } = createDispatchClient();

    await startOnboarding(client, "nsec", "  nsec1secretsecret  ");

    expect(calls).toEqual([
      {
        actionType: "chirp.identity.onboard",
        payload: { mode: "nsec", credential_hint: "nsec1..." },
      },
    ]);
  });

  it("normalizes user-entered text payloads", async () => {
    const { calls, client } = createDispatchClient();

    await connectNip46(client, "  bunker://remote  ");
    await submitNote(client, "  hello nostr  ");
    await search(client, "  alice  ");
    await sendGroupMessage(client, "group-1", "  gm  ");
    await sendDirectMessage(client, "alice", "  encrypted hello  ");

    expect(calls).toEqual([
      { actionType: "chirp.identity.nip46.connect", payload: { bunker_uri: "bunker://remote" } },
      { actionType: "chirp.compose.submit", payload: { text: "hello nostr" } },
      { actionType: "chirp.search.query", payload: { query: "alice" } },
      { actionType: "chirp.groups.message", payload: { group_id: "group-1", text: "gm" } },
      { actionType: "chirp.dm.send", payload: { pubkey: "alice", text: "encrypted hello" } },
    ]);
  });

  it("dispatches social, entity, group, timeline, and relay actions", async () => {
    const { calls, client } = createDispatchClient();

    await requestProfile(client, "alice");
    await setFollow(client, "alice", true);
    await setFollow(client, "alice", false);
    await openBech32(client, "nevent1qq");
    await refreshTimeline(client, "home");
    await joinGroup(client, "g1");
    await openGroup(client, "g1");
    await saveRelays(client, ["wss://relay.example"]);

    expect(calls).toEqual([
      { actionType: "chirp.profile.open", payload: { pubkey: "alice" } },
      { actionType: "chirp.profile.follow", payload: { pubkey: "alice" } },
      { actionType: "chirp.profile.unfollow", payload: { pubkey: "alice" } },
      { actionType: "chirp.entity.open", payload: { entity: "nevent1qq" } },
      { actionType: "chirp.timeline.refresh", payload: { surface: "home" } },
      { actionType: "chirp.groups.join", payload: { group_id: "g1" } },
      { actionType: "chirp.groups.open", payload: { group_id: "g1" } },
      { actionType: "chirp.relays.save", payload: { relays: ["wss://relay.example"] } },
    ]);
  });
});
