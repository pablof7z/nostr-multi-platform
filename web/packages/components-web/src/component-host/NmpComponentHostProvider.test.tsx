import { renderToString } from "solid-js/web";
import { describe, expect, it } from "vitest";
import { NostrContentView } from "../content-view/NostrContentView";
import { NostrAvatar } from "../user-avatar/NostrAvatar";
import { NmpComponentHostProvider } from "./NmpComponentHostProvider";
import {
  COMPONENT_HOST_FIXTURE_EVENT_ID,
  COMPONENT_HOST_FIXTURE_PUBKEY,
  componentHostEventRefTree,
  createComponentHostConformanceFixture,
} from "./conformanceFixtures";

describe("NmpComponentHostProvider conformance", () => {
  it("renders user and event components from fake host fixtures without a runtime", () => {
    const fixture = createComponentHostConformanceFixture();

    const html = renderToString(() => (
      <NmpComponentHostProvider
        profileHost={fixture.profileHost}
        resolvedEventEmbeds={fixture.resolvedEventEmbeds}
        eventRefResolver={fixture.eventRefResolver}
      >
        <NostrAvatar pubkey={COMPONENT_HOST_FIXTURE_PUBKEY} consumerId="fixture.avatar" />
        <NostrContentView tree={componentHostEventRefTree()} nowSeconds={1_700_000_060} />
      </NmpComponentHostProvider>
    ));

    expect(html).toContain("https://example.invalid/alice.png");
    expect(html).toContain("nostr-event-ref-embed");
    expect(html).toContain(`data-primary-id="${COMPONENT_HOST_FIXTURE_EVENT_ID}"`);
    expect(html).toContain("nostr-quote-card");
    expect(html).toContain("Conformance Alice");
    expect(html).toContain("Event render data supplied by refs.event.envelopes.");
  });
});
