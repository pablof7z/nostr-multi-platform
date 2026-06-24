/** Builds a unique worker-request correlation id.
 *
 * `Date.now()` has millisecond resolution, so two calls in the same tick
 * can produce the same string. The caller-supplied monotonic `seq` makes the
 * id unique regardless of timing.
 */
export function makeCorrelationId(prefix: string, seq: number): string {
  return `${prefix}-${Date.now()}-${seq}`;
}
