// @nmp/components — shared NMP SolidJS component library.
// Single source of truth: apps import, never copy. The registry displays
// these files as source; both gallery and chirp render them live.

// user-avatar group
export type { ProfileWire } from "./user-avatar/ProfileWire";
export { avatarUrl, displayLabel, shortHex } from "./user-avatar/ProfileWire";
export type { NostrProfileHost } from "./user-avatar/NostrProfileHost";
export {
  NostrProfileHostProvider,
  useNostrProfileHost,
} from "./user-avatar/NostrProfileHost";
export { NostrAvatar, identiconColor, identiconInitials } from "./user-avatar/NostrAvatar";

// user-card group
export { NostrUserCard } from "./user-card/NostrUserCard";

// user-name group
export { NostrProfileName } from "./user-name/NostrProfileName";

// user-nip05 group
export { NostrNip05Badge } from "./user-nip05/NostrNip05Badge";

// user-npub group
export { NostrNpubChip } from "./user-npub/NostrNpubChip";

// content-core group
export {
  decodeContentTree,
  isTreeRenderable,
  ContentTreeWire,
  WireNodeKind,
} from "./content-core/decodeContentTree";
export type { WireNode } from "./content-core/decodeContentTree";

// content-view group
export { NostrContentView } from "./content-view/NostrContentView";
export type { NostrContentViewProps } from "./content-view/NostrContentView";

// content-minimal group
export { NostrMinimalContentView } from "./content-minimal/NostrMinimalContentView";

// content-mention-chip group
export { NostrMentionChip } from "./content-mention-chip/NostrMentionChip";

// content-kind-* group
export { NostrArticleCard } from "./content-kind-30023/NostrArticleCard";
export type { NostrArticleCardModel } from "./content-kind-30023/NostrArticleCard";
export { NostrHighlightCard } from "./content-kind-9802/NostrHighlightCard";
export type { NostrHighlightCardModel } from "./content-kind-9802/NostrHighlightCard";
export { NostrQuoteCard, relativeTime } from "./content-quote-card/NostrQuoteCard";
export type { NostrQuoteCardModel } from "./content-quote-card/NostrQuoteCard";
export { NostrKindRegistry } from "./content-kind-registry/NostrKindRegistry";
export type { EmbeddedEventModel } from "./content-kind-registry/NostrKindRegistry";

// content-media-grid group
export { NostrMediaGrid } from "./content-media-grid/NostrMediaGrid";

// login-block group
export { NostrLoginBlock } from "./login-block/NostrLoginBlock";

// relay-list group
export { NostrRelayList } from "./relay-list/NostrRelayList";
