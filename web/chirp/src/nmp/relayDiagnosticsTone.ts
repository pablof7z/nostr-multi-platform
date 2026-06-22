/**
 * Shell-side tone policy (#1768): derive a semantic hue token from the RAW
 * protocol tokens the kernel emits on relay statuses / logical interests /
 * wire subscriptions. The kernel emits only raw `role` / `connection` /
 * `auth` / `state` strings (and reason `kind`); deciding which hue class each
 * belongs to is the app's job. These tokens feed the `ins-dot-*` / `ins-chip-*`
 * CSS classes.
 *
 * Ported verbatim from the former kernel `relay_diagnostics/format.rs`
 * selectors (which were deleted when tone left the wire). Only the selectors
 * the web inspector actually renders live here (connection / auth / wire-sub
 * state / interest state); role + reason hue have no web surface today.
 */

/** Relay connection → tone. */
export function connectionTone(connection: string): string {
  const lower = connection.toLowerCase();
  if (lower === "connected") return "ok";
  if (lower.startsWith("disconnect") || lower === "failed") return "error";
  if (lower.includes("connect")) return "warn";
  if (lower === "unknown" || lower === "idle" || lower === "—" || lower === "blocked") {
    return "muted";
  }
  return "error";
}

/** Relay auth → tone. `null` / empty auth → muted. */
export function authTone(auth: string | null | undefined): string {
  const lower = (auth ?? "").toLowerCase();
  if (lower === "ok" || lower === "authenticated") return "ok";
  if (lower === "pending") return "warn";
  return "muted";
}

/** Wire-subscription state → tone. */
export function wireSubStateTone(state: string): string {
  switch (state.toLowerCase()) {
    case "open":
    case "active":
    case "live":
      return "ok";
    case "pending":
    case "warming":
    case "opening":
    case "auth_paused":
      return "warn";
    default:
      return "muted";
  }
}

/** Logical-interest state → tone. */
export function interestStateTone(state: string): string {
  switch (state) {
    case "active":
    case "warming":
    case "tailing":
    case "complete":
      return "ok";
    case "idle":
      return "muted";
    default:
      return "warn";
  }
}
