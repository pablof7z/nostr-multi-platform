import { A, useLocation } from "@solidjs/router";

type Props = {
  onToggleSidebar: () => void;
};

export default function Topbar(props: Props) {
  const location = useLocation();
  const is = (path: string) =>
    location.pathname === path ? ("page" as const) : undefined;

  return (
    <header class="topbar">
      <div style="display: flex; align-items: center; gap: 0.75rem;">
        <button
          type="button"
          class="topbar__menu-btn"
          onClick={props.onToggleSidebar}
          aria-label="Toggle component menu"
        >
          Menu
        </button>
        <A href="/" class="topbar__brand">
          <span class="topbar__brand-mark">nmp</span>
          <span>registry</span>
        </A>
      </div>
      <nav class="topbar__nav" aria-label="Primary">
        <A href="/" aria-current={is("/")}>
          Home
        </A>
        <A href="/get-started" aria-current={is("/get-started")}>
          Get started
        </A>
        <A href="/components/content-core" aria-current={is("/components/content-core")}>
          Components
        </A>
        <a
          href="https://github.com/pablof7z/nostr-multi-platform"
          target="_blank"
          rel="noreferrer noopener"
        >
          GitHub
        </a>
      </nav>
    </header>
  );
}
