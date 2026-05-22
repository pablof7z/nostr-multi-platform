export type ContentToken =
  | { type: "text"; value: string }
  | { type: "entity"; value: string; entityType: Bech32EntityType };

export type Bech32EntityType =
  | "profile"
  | "note"
  | "event"
  | "address"
  | "relay"
  | "secret"
  | "unknown";

const bech32Pattern =
  /\b(npub1[023456789acdefghjklmnpqrstuvwxyz]+|nprofile1[023456789acdefghjklmnpqrstuvwxyz]+|note1[023456789acdefghjklmnpqrstuvwxyz]+|nevent1[023456789acdefghjklmnpqrstuvwxyz]+|naddr1[023456789acdefghjklmnpqrstuvwxyz]+|nrelay1[023456789acdefghjklmnpqrstuvwxyz]+|nsec1[023456789acdefghjklmnpqrstuvwxyz]+)/gi;

export function tokenizeContent(content: string): ContentToken[] {
  const tokens: ContentToken[] = [];
  let cursor = 0;
  for (const match of content.matchAll(bech32Pattern)) {
    const index = match.index ?? 0;
    if (index > cursor) {
      tokens.push({ type: "text", value: content.slice(cursor, index) });
    }
    const value = match[0];
    tokens.push({ type: "entity", value, entityType: classifyEntity(value) });
    cursor = index + value.length;
  }
  if (cursor < content.length) {
    tokens.push({ type: "text", value: content.slice(cursor) });
  }
  return tokens.length > 0 ? tokens : [{ type: "text", value: content }];
}

export function classifyEntity(value: string): Bech32EntityType {
  const lower = value.toLowerCase();
  if (lower.startsWith("npub1") || lower.startsWith("nprofile1")) {
    return "profile";
  }
  if (lower.startsWith("note1")) {
    return "note";
  }
  if (lower.startsWith("nevent1")) {
    return "event";
  }
  if (lower.startsWith("naddr1")) {
    return "address";
  }
  if (lower.startsWith("nrelay1")) {
    return "relay";
  }
  if (lower.startsWith("nsec1")) {
    return "secret";
  }
  return "unknown";
}

export function shortEntity(value: string) {
  if (value.length <= 18) {
    return value;
  }
  return `${value.slice(0, 10)}...${value.slice(-6)}`;
}
