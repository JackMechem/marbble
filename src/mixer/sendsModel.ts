// What a channel strip's send slots already use: pure, so the tests can run it (routing.ts builds
// the menus).
import type { Id, Send } from "../engine/project";

/** The buses a track's other send slots already send to: a second send to one would only add
 *  to the first, so the slot menus grey them out, as Logic's do. */
export function busesInUse(sends: (Send | null)[] | undefined, slot: number): Id[] {
  return (sends ?? []).flatMap((s, i) => (s && i !== slot ? [s.bus] : []));
}
