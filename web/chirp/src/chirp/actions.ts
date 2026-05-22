import type { NmpClient } from "../nmp/client";
import type { OnboardingMode } from "./model";

export function startOnboarding(client: NmpClient, mode: OnboardingMode, value: string) {
  return client.dispatch("chirp.identity.onboard", {
    mode,
    credential_hint: redactSecretLikeValue(value),
  });
}

export function connectNip46(client: NmpClient, bunkerUri: string) {
  return client.dispatch("chirp.identity.nip46.connect", {
    bunker_uri: bunkerUri.trim(),
  });
}

export function requestProfile(client: NmpClient, pubkey: string) {
  return client.dispatch("chirp.profile.open", { pubkey });
}

export function setFollow(client: NmpClient, pubkey: string, follow: boolean) {
  return client.dispatch(follow ? "chirp.profile.follow" : "chirp.profile.unfollow", {
    pubkey,
  });
}

export function submitNote(client: NmpClient, text: string) {
  return client.dispatch("chirp.compose.submit", { text: text.trim() });
}

export function refreshTimeline(client: NmpClient, surface: string) {
  return client.dispatch("chirp.timeline.refresh", { surface });
}

export function openBech32(client: NmpClient, entity: string) {
  return client.dispatch("chirp.entity.open", { entity });
}

export function search(client: NmpClient, query: string) {
  return client.dispatch("chirp.search.query", { query: query.trim() });
}

export function joinGroup(client: NmpClient, groupId: string) {
  return client.dispatch("chirp.groups.join", { group_id: groupId });
}

export function openGroup(client: NmpClient, groupId: string) {
  return client.dispatch("chirp.groups.open", { group_id: groupId });
}

export function sendGroupMessage(client: NmpClient, groupId: string, text: string) {
  return client.dispatch("chirp.groups.message", { group_id: groupId, text: text.trim() });
}

export function sendDirectMessage(client: NmpClient, pubkey: string, text: string) {
  return client.dispatch("chirp.dm.send", { pubkey, text: text.trim() });
}

export function saveRelays(client: NmpClient, relays: string[]) {
  return client.dispatch("chirp.relays.save", { relays });
}

function redactSecretLikeValue(value: string) {
  const trimmed = value.trim();
  if (trimmed.startsWith("nsec1")) {
    return "nsec1...";
  }
  return trimmed;
}
