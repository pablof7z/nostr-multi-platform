import { expect, test } from "@playwright/test";
import { startGroupFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm groups workspace renders NIP-29 discovery from Rust projection", async ({ page }) => {
  test.setTimeout(90_000);

  const relay = await startGroupFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const url = `/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}&group_relay=${encodeURIComponent(relay.url)}#groups`;

  try {
    await page.goto(url);

    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await expect(shell).toHaveAttribute("data-has-snapshot", "true", { timeout: 30_000 });
    const panel = page.getByTestId("groups-panel");
    await expect(panel).toBeVisible();
    await expect(panel.getByRole("heading", { name: "Public groups", exact: true })).toBeVisible();

    const card = page.getByTestId("group-card").filter({ hasText: relay.groupName });
    await expect(card).toBeVisible({ timeout: 30_000 });
    await expect(card).toContainText(relay.groupId);
    await expect(card).toContainText(relay.groupAbout);
    await expect(card).toContainText(`${relay.memberCount} members`);
    await expect(card).toContainText(`${relay.adminCount} admins`);
    await expect(card).toContainText("public");
    await expect(card).toContainText("open");
    await expect(card).toContainText("Open timeline");
    await expect(card).toContainText("Inspect join");

    await card.getByTestId("group-timeline-open").click();
    const timeline = page.getByTestId("group-timeline-panel");
    await expect(timeline).toBeVisible();
    await expect(timeline).toContainText(relay.groupName);
    await expect(timeline.getByTestId("group-timeline-row")).toContainText(
      relay.groupMessageContent,
      { timeout: 30_000 },
    );
    await expect(timeline.getByTestId("group-timeline-row")).toContainText("kind 9");

    await card.getByTestId("group-join-inspect").click();
    await expect(page.getByTestId("groups-diagnostic")).toContainText("nmp.nip29.join");

    await expect
      .poll(() => relay.subscriptions().some((filter) => includesGroupMetadataKinds(filter.kinds)))
      .toBe(true);
    await expect
      .poll(() => relay.subscriptions().some((filter) => includesGroupEventsFilter(filter, relay.groupId)))
      .toBe(true);
  } finally {
    await relay.close();
  }
});

function includesGroupMetadataKinds(kinds: number[] | undefined): boolean {
  return (
    Array.isArray(kinds) &&
    [39000, 39001, 39002].every((kind) => kinds.includes(kind))
  );
}

function includesGroupEventsFilter(filter: Record<string, unknown>, groupId: string): boolean {
  const kinds = filter.kinds;
  const hTags = filter["#h"];
  return (
    Array.isArray(kinds) &&
    kinds.includes(9) &&
    kinds.includes(11) &&
    Array.isArray(hTags) &&
    hTags.includes(groupId)
  );
}
