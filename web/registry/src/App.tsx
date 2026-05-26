import { JSX, Show, createSignal } from "solid-js";
import { Route, Router, RouteSectionProps, useLocation } from "@solidjs/router";
import Sidebar from "./components/Sidebar";
import Topbar from "./components/Topbar";
import Landing from "./pages/index";
import GetStarted from "./pages/get-started";
import ComponentPage from "./pages/ComponentPage";
import NotFound from "./pages/not-found";

function Shell(props: RouteSectionProps): JSX.Element {
  const location = useLocation();
  const isHome = () => location.pathname === "/";
  const [sidebarOpen, setSidebarOpen] = createSignal(false);

  return (
    <div class="app" classList={{ "app--wide": isHome() }}>
      <Topbar onToggleSidebar={() => setSidebarOpen((v) => !v)} isHome={isHome()} />
      <Show when={!isHome()}>
        <Sidebar open={sidebarOpen()} />
      </Show>
      <main aria-label="Main content" style="min-width: 0;">
        {props.children}
      </main>
    </div>
  );
}

export default function App() {
  return (
    <Router root={Shell}>
      <Route path="/" component={Landing} />
      <Route path="/get-started" component={GetStarted} />
      <Route path="/components/:id" component={ComponentPage} />
      <Route path="*" component={NotFound} />
    </Router>
  );
}
