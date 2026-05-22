import type { Component } from "solid-js";
import {
  Compass,
  Home,
  MessagesSquare,
  Radio,
  Search,
  Settings,
  ShieldCheck,
  UserRound,
  UsersRound,
  Wallet,
} from "lucide-solid";

export type ClientView =
  | "home"
  | "profile"
  | "messages"
  | "groups"
  | "wallet"
  | "settings"
  | "search";

export type NavItem = {
  id: ClientView;
  label: string;
  Icon: Component<{ size?: number }>;
};

export const navItems: NavItem[] = [
  { id: "home", label: "Home", Icon: Home },
  { id: "messages", label: "Chats", Icon: MessagesSquare },
  { id: "groups", label: "Groups", Icon: UsersRound },
  { id: "wallet", label: "Wallet", Icon: Wallet },
  { id: "settings", label: "Settings", Icon: Settings },
  { id: "search", label: "Search", Icon: Search },
  { id: "profile", label: "Profile", Icon: UserRound },
];

export type OnboardingMode = "create" | "nsec" | "nip46" | "nip07";

export type ProfileSummary = {
  pubkey: string;
  name: string;
  handle: string;
  about: string;
  avatarUrl?: string;
  bannerUrl?: string;
  followsYou?: boolean;
  followedByViewer?: boolean;
  stats: {
    following: number;
    followers: number;
    notes: number;
  };
};

export type TimelineItem = {
  id: string;
  author: ProfileSummary;
  createdAt: string;
  content: string;
  replyCount: number;
  repostCount: number;
  reactionCount: number;
};

export type GroupSummary = {
  id: string;
  name: string;
  host: string;
  description: string;
  memberCount: number;
  joined: boolean;
};

export const activeProfile: ProfileSummary = {
  pubkey: "npub1pablof7zwebclientproof0000000000000000000000000000000",
  name: "Pablo",
  handle: "pablof7z",
  about: "Building Chirp as the full NMP showcase client.",
  avatarUrl: "https://api.dicebear.com/9.x/shapes/svg?seed=pablof7z",
  bannerUrl: "https://api.dicebear.com/9.x/glass/svg?seed=chirp",
  followsYou: false,
  followedByViewer: true,
  stats: { following: 184, followers: 1400, notes: 862 },
};

export const showcaseProfiles: ProfileSummary[] = [
  activeProfile,
  {
    pubkey: "npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft",
    name: "NMP Relay Monitor",
    handle: "relay-monitor",
    about: "Publishes relay health, kind:10002 changes, and follow-feed routing evidence.",
    avatarUrl: "https://api.dicebear.com/9.x/shapes/svg?seed=relay-monitor",
    followsYou: true,
    followedByViewer: true,
    stats: { following: 12, followers: 893, notes: 520 },
  },
  {
    pubkey: "npub1fiatjafwebclientproof00000000000000000000000000000000",
    name: "fiatjaf",
    handle: "fiatjaf",
    about: "Nostr protocol notes and client interoperability checks.",
    avatarUrl: "https://api.dicebear.com/9.x/shapes/svg?seed=fiatjaf",
    followsYou: false,
    followedByViewer: false,
    stats: { following: 331, followers: 98100, notes: 4180 },
  },
];

export const timelineItems: TimelineItem[] = [
  {
    id: "note1chirpwebfullclient0000000000000000000000000000000000",
    author: showcaseProfiles[1],
    createdAt: "now",
    content:
      "Chirp Web now has the same top-level surfaces as iOS: onboarding, NIP-46, profiles, groups, DMs, settings, and bech32 cards like note1w3bproof0000000000000000000000000000000000000000.",
    replyCount: 4,
    repostCount: 12,
    reactionCount: 38,
  },
  {
    id: "note1nip46webclient000000000000000000000000000000000000",
    author: showcaseProfiles[2],
    createdAt: "4m",
    content:
      "Remote signer login belongs behind the NMP signer capability. Try bunker:// and nostrconnect:// in onboarding; web should execute capabilities and Rust should own policy.",
    replyCount: 9,
    repostCount: 21,
    reactionCount: 91,
  },
];

export const groups: GroupSummary[] = [
  {
    id: "naddr1chirpgroupsweb000000000000000000000000000000000000",
    name: "NMP Builders",
    host: "wss://relay.nostr.net",
    description: "NIP-29 group chat for implementation notes and protocol smoke checks.",
    memberCount: 128,
    joined: true,
  },
  {
    id: "naddr1marmotwebproof000000000000000000000000000000000000",
    name: "Marmot MLS",
    host: "wss://groups.0xchat.com",
    description: "Encrypted group experiments surfaced through reusable NMP/Marmot projections.",
    memberCount: 42,
    joined: false,
  },
];

export const capabilitySurfaces = [
  { label: "NIP-46 remote signer", Icon: ShieldCheck },
  { label: "Relay diagnostics", Icon: Radio },
  { label: "NIP-29 discovery", Icon: Compass },
];
