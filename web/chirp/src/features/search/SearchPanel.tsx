import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { CHIRP_SEARCH_RELAY_URL, chirpSearchRelayUrlsFromSearch } from "../../chirpConfig";
import { useNmpClient } from "../../nmp/context";
import { decodeSearchResultsFrame, type SearchResultRow } from "../../nmp/searchDecoder";
import "./search.css";

type SearchScope = "notes" | "profiles" | "longform";

const SESSION_ID = "chirp-web-main";
const DEFAULT_QUERY = "nostr";

function hashSearchParams(): { query: string; scope: SearchScope } {
  const [, rawQuery = ""] = window.location.hash.split("?");
  const params = new URLSearchParams(rawQuery);
  const scope = params.get("scope");
  return {
    query: params.get("q")?.trim() || DEFAULT_QUERY,
    scope: scope === "profiles" || scope === "longform" ? scope : "notes",
  };
}

export function SearchPanel() {
  const { client, snapshot } = useNmpClient();
  const initial = hashSearchParams();
  const [query, setQuery] = createSignal(initial.query);
  const [scope, setScope] = createSignal<SearchScope>(initial.scope);
  const [lastQuery, setLastQuery] = createSignal("");
  const [isSearching, setIsSearching] = createSignal(false);
  const [autoRan, setAutoRan] = createSignal(false);
  const [rows, setRows] = createSignal<SearchResultRow[]>([]);
  const searchRelays =
    chirpSearchRelayUrlsFromSearch(window.location.search) ?? [CHIRP_SEARCH_RELAY_URL];

  const decodedFrame = createMemo(() =>
    decodeSearchResultsFrame(snapshot().latestUpdateBytes, SESSION_ID),
  );
  createEffect(() => {
    const frame = decodedFrame();
    if (frame) setRows(frame.rows);
  });
  const resultCount = () => rows().length;

  createEffect(() => {
    if (autoRan()) return;
    if (snapshot().status !== "running") return;
    setAutoRan(true);
    void runSearch(query(), scope());
  });

  onCleanup(() => {
    void client.closeSearch(SESSION_ID);
  });

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    void runSearch(query(), scope());
  };

  onMount(() => {
    const syncHashQuery = () => {
      const next = hashSearchParams();
      if (next.query === query() && next.scope === scope()) return;
      setQuery(next.query);
      setScope(next.scope);
      if (snapshot().status === "running") void runSearch(next.query, next.scope);
    };
    window.addEventListener("hashchange", syncHashQuery);
    onCleanup(() => window.removeEventListener("hashchange", syncHashQuery));
  });

  const selectScope = (next: SearchScope) => {
    setScope(next);
    if (lastQuery()) void runSearch(query(), next);
  };

  async function runSearch(rawQuery: string, nextScope: SearchScope) {
    const normalized = rawQuery.trim();
    if (!normalized) return;
    setIsSearching(true);
    setLastQuery(normalized);
    try {
      await client.openSearch({
        sessionId: SESSION_ID,
        query: normalized,
        scope: nextScope,
        targets: "explicit",
        relays: searchRelays,
        maxHits: 80,
      });
    } finally {
      setIsSearching(false);
    }
  }

  return (
    <section class="search-panel" data-testid="search-panel" aria-label="Search">
      <form class="search-box" onSubmit={submit}>
        <div class="search-field">
          <label for="chirp-search-input">Search Nostr</label>
          <input
            id="chirp-search-input"
            data-testid="search-input"
            value={query()}
            onInput={(event) => setQuery(event.currentTarget.value)}
            placeholder="Search notes, profiles, and long-form posts"
            autocomplete="off"
          />
        </div>
        <button class="search-submit" type="submit" data-testid="search-submit">
          Search
        </button>
      </form>

      <div class="search-toolbar">
        <div class="search-tabs" role="tablist" aria-label="Search scopes">
          <ScopeButton label="Notes" value="notes" active={scope()} onSelect={selectScope} />
          <ScopeButton label="Profiles" value="profiles" active={scope()} onSelect={selectScope} />
          <ScopeButton label="Long-form" value="longform" active={scope()} onSelect={selectScope} />
        </div>
        <span class="search-source" title={searchRelays.join(", ")}>
          {isSearching() ? "searching" : `${resultCount()} results`} ·{" "}
          {searchRelays[0]?.replace(/^wss?:\/\//, "") ?? "no relay"}
        </span>
      </div>

      <div class="search-results" data-testid="search-results">
        <Show
          when={resultCount() > 0}
          fallback={
            <div class="search-empty" data-testid="search-empty">
              <strong>{lastQuery() ? "No matching events yet" : "Search is ready"}</strong>
              <span>
                {lastQuery()
                  ? "Matching relay or cached hits will appear here."
                  : "Run a search to find notes, profiles, or long-form posts."}
              </span>
            </div>
          }
        >
          <For each={rows()}>
            {(row) => <SearchResultCard row={row} />}
          </For>
        </Show>
      </div>
    </section>
  );
}

function ScopeButton(props: {
  label: string;
  value: SearchScope;
  active: SearchScope;
  onSelect: (scope: SearchScope) => void;
}) {
  const selected = () => props.active === props.value;
  return (
    <button
      type="button"
      role="tab"
      aria-selected={selected() ? "true" : "false"}
      data-active={selected() ? "true" : "false"}
      onClick={() => props.onSelect(props.value)}
    >
      {props.label}
    </button>
  );
}

function SearchResultCard(props: { row: SearchResultRow }) {
  const timestamp = () =>
    new Date(props.row.createdAt * 1000).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  const source = () =>
    props.row.source === "cache"
      ? cacheSourceLabel(props.row.relayProvenance)
      : (props.row.sourceRelay ?? props.row.relayProvenance[0] ?? "relay").replace(/^wss?:\/\//, "");
  return (
    <article
      class="search-result-card"
      data-testid="search-result-card"
      data-event-id={props.row.id}
    >
      <div class="search-result-header">
        <strong>{shortHex(props.row.authorPubkey)}</strong>
        <span>kind {props.row.kind}</span>
        <time>{timestamp()}</time>
      </div>
      <p>{props.row.content || metadataSummary(props.row.tags)}</p>
      <div class="search-result-meta">
        <span title={props.row.id}>{shortHex(props.row.id)}</span>
        <span title={source()}>{source()}</span>
      </div>
    </article>
  );
}

function cacheSourceLabel(relayProvenance: string[]): string {
  const relay = relayProvenance[0]?.replace(/^wss?:\/\//, "");
  return relay ? `cache · ${relay}` : "cache";
}

function shortHex(value: string): string {
  return value.length <= 16 ? value : `${value.slice(0, 8)}...${value.slice(-6)}`;
}

function metadataSummary(tags: string[][]): string {
  const name = tags.find((tag) => tag[0] === "name")?.[1];
  const displayName = tags.find((tag) => tag[0] === "display_name")?.[1];
  return displayName || name || "Profile metadata event";
}
