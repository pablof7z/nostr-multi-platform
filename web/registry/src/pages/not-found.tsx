import { A } from "@solidjs/router";
import PageMeta from "../components/PageMeta";

export default function NotFound() {
  return (
    <div class="content">
      <PageMeta
        title="Not found — NMP Registry"
        description="The page you're looking for doesn't exist."
      />
      <h1>404</h1>
      <p class="lead">That page doesn't exist.</p>
      <p>
        <A href="/" class="btn">
          Back to home
        </A>
      </p>
    </div>
  );
}
