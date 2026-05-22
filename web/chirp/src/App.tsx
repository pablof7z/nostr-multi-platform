import { Match, Switch, createSignal, onCleanup } from "solid-js";
import {
  connectNip46,
  joinGroup,
  openBech32,
  openGroup,
  refreshTimeline,
  requestProfile,
  sendGroupMessage,
  setFollow,
  startOnboarding,
  submitNote,
} from "./chirp/actions";
import {
  activeProfile,
  groups,
  showcaseProfiles,
  timelineItems,
  type ClientView,
  type ProfileSummary,
} from "./chirp/model";
import { createNmpClient, type RuntimeSnapshot } from "./nmp/client";
import { GroupsView } from "./features/Groups";
import { Inspector } from "./features/Inspector";
import { Onboarding } from "./features/Onboarding";
import { ProfileView } from "./features/Profile";
import { Shell } from "./features/Shell";
import { TimelineView } from "./features/Timeline";
import { MentionsView, MessagesView, SearchView, SettingsView, WalletView } from "./features/UtilityViews";
import { runtimeConnection } from "./nmp/client";

const client = createNmpClient();

export default function App() {
  const [snapshot, setSnapshot] = createSignal<RuntimeSnapshot>(client.snapshot());
  const [starting, setStarting] = createSignal(false);
  const [draft, setDraft] = createSignal("");
  const [view, setView] = createSignal<ClientView>("home");
  const [profile, setProfile] = createSignal<ProfileSummary>(activeProfile);
  const unsubscribe = client.subscribe(setSnapshot);

  onCleanup(unsubscribe);

  const start = async () => {
    setStarting(true);
    setSnapshot(await client.start());
    setStarting(false);
  };

  const openProfile = async (pubkey: string) => {
    const next = showcaseProfiles.find((candidate) => candidate.pubkey === pubkey) ?? profile();
    setProfile(next);
    setView("profile");
    setSnapshot(await requestProfile(client, pubkey));
  };

  const publish = async () => {
    const text = draft().trim();
    if (!text) {
      return;
    }
    setSnapshot(await submitNote(client, text));
    setDraft("");
  };

  return (
    <Shell
      view={view()}
      onView={setView}
      aside={<Inspector snapshot={snapshot()} starting={starting()} onStart={start} />}
    >
      <Content
        view={view()}
        profile={profile()}
        draft={draft()}
        onDraft={setDraft}
        onPublish={publish}
        onProfile={openProfile}
        onSnapshot={setSnapshot}
        onView={setView}
      />
    </Shell>
  );
}

function Content(props: {
  view: ClientView;
  profile: ProfileSummary;
  draft: string;
  onDraft: (value: string) => void;
  onPublish: () => void;
  onProfile: (pubkey: string) => void;
  onSnapshot: (snapshot: RuntimeSnapshot) => void;
  onView: (view: ClientView) => void;
}) {
  const entity = async (value: string) => props.onSnapshot(await openBech32(client, value));
  const follow = async (pubkey: string, enabled: boolean) => {
    props.onSnapshot(await setFollow(client, pubkey, enabled));
  };

  return (
    <Switch>
      <Match when={props.view === "home"}>
        <>
          <Onboarding
            onStart={async (mode, value) => props.onSnapshot(await startOnboarding(client, mode, value))}
            onNip46={async (value) => props.onSnapshot(await connectNip46(client, value))}
          />
          <TimelineView
            eyebrow="Home"
            title="Following timeline"
            items={timelineItems}
            draft={props.draft}
            onDraft={props.onDraft}
            onPublish={props.onPublish}
            onRefresh={async () => props.onSnapshot(await refreshTimeline(client, "home"))}
            onProfile={props.onProfile}
            onEntity={entity}
          />
        </>
      </Match>
      <Match when={props.view === "profile"}>
        <ProfileView
          profile={props.profile}
          notes={timelineItems.filter((item) => item.author.pubkey === props.profile.pubkey)}
          onFollow={follow}
          onEntity={entity}
        />
      </Match>
      <Match when={props.view === "messages"}>
        <MessagesView client={client} onProfile={props.onProfile} />
      </Match>
      <Match when={props.view === "groups"}>
        <GroupsView
          groups={groups}
          onJoin={async (id) => props.onSnapshot(await joinGroup(client, id))}
          onOpen={async (id) => props.onSnapshot(await openGroup(client, id))}
          onSend={async (id, text) => props.onSnapshot(await sendGroupMessage(client, id, text))}
        />
      </Match>
      <Match when={props.view === "wallet"}>
        <WalletView />
      </Match>
      <Match when={props.view === "settings"}>
        <SettingsView client={client} relays={runtimeConnection.relays} />
      </Match>
      <Match when={props.view === "search"}>
        <>
          <SearchView client={client} onProfile={props.onProfile} />
          <MentionsView onEntity={entity} />
        </>
      </Match>
    </Switch>
  );
}
