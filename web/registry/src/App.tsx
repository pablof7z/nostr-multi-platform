import { JSX, createSignal } from "solid-js";
import { Route, Router, RouteSectionProps } from "@solidjs/router";
import Sidebar from "./components/Sidebar";
import Topbar from "./components/Topbar";
import Landing from "./pages/index";
import GetStarted from "./pages/get-started";
import ComponentPage from "./pages/ComponentPage";
import NotFound from "./pages/not-found";

/**
 * Top-level layout. The topbar spans both columns; the sidebar + main
 * content sit underneath in a 2-column grid (collapsing to 1 column on
 * narrow viewports — see `global.css` media query).
 */
function Shell(props: RouteSectionProps): JSX.Element {
  const [sidebarOpen, setSidebarOpen] = createSignal(false);
  return (
    <div class="app">
      <Topbar onToggleSidebar={() => setSidebarOpen((v) => !v)} />
      <Sidebar open={sidebarOpen()} />
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
