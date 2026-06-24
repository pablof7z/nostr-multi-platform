import type { ProfileWire } from "../components/user-avatar/ProfileWire";

/** Structural equality of materialised `refs.profile` maps.
 *
 * A no-op frame keeps the same map reference so feed rows do not churn. Any
 * content change, including an identity/epoch rebaseline that clears rows,
 * returns false so the client swaps to the new profile set.
 */
export function profileCardsEqual(
  a: Map<string, ProfileWire> | undefined,
  b: Map<string, ProfileWire>,
): boolean {
  if (a === undefined || a.size !== b.size) return false;
  for (const [key, wa] of a) {
    const wb = b.get(key);
    if (
      !wb ||
      wa.displayName !== wb.displayName ||
      wa.pictureUrl !== wb.pictureUrl ||
      wa.nip05 !== wb.nip05 ||
      wa.about !== wb.about ||
      wa.lnurl !== wb.lnurl ||
      wa.npubShort !== wb.npubShort
    ) {
      return false;
    }
  }
  return true;
}
