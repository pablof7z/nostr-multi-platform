import { createContext, useContext, type Accessor, type JSX } from "solid-js";
import type { EmbeddedEventModel } from "../content-kind-registry/NostrKindRegistry";

export type ResolvedEventEmbeds = ReadonlyMap<string, EmbeddedEventModel>;
export type ResolvedEventEmbedsInput = ResolvedEventEmbeds | Accessor<ResolvedEventEmbeds>;

const EMPTY_EMBEDS: ResolvedEventEmbeds = new Map();

const ResolvedEventEmbedsContext = createContext<Accessor<ResolvedEventEmbeds>>();

function asAccessor(input: ResolvedEventEmbedsInput): Accessor<ResolvedEventEmbeds> {
  return typeof input === "function" ? input : () => input;
}

export function ResolvedEventEmbedsProvider(props: {
  resolvedEventEmbeds: ResolvedEventEmbedsInput;
  children: JSX.Element;
}): JSX.Element {
  return (
    <ResolvedEventEmbedsContext.Provider value={asAccessor(props.resolvedEventEmbeds)}>
      {props.children}
    </ResolvedEventEmbedsContext.Provider>
  );
}

export function useOptionalResolvedEventEmbeds(): Accessor<ResolvedEventEmbeds> | undefined {
  return useContext(ResolvedEventEmbedsContext);
}

export function useResolvedEventEmbeds(): Accessor<ResolvedEventEmbeds> {
  return useOptionalResolvedEventEmbeds() ?? (() => EMPTY_EMBEDS);
}

export function useResolvedEventEmbed(
  primaryId: Accessor<string | undefined>,
): Accessor<EmbeddedEventModel | undefined> {
  const embeds = useResolvedEventEmbeds();
  return () => {
    const key = primaryId();
    return key ? embeds().get(key) : undefined;
  };
}
