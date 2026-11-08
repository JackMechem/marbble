// Take ratings: what a take row and the take folder menu show for a take marked good or bad,
// and what choosing a rating does. The ratings are kept in the engine (`Take::rating`). Pure,
// so the tests can import it.
import type { Take, TakeFolder, TakeRating } from "./api";

/** The mark before a rated take's name. */
export const RATING_MARKS: Record<TakeRating, string> = { none: "", good: "★", bad: "✗" };

/** A take's name with its mark: "★ Take 2". */
export function takeLabel(t: Take): string {
  const mark = RATING_MARKS[t.rating ?? "none"];
  return mark ? `${mark} ${t.name}` : t.name;
}

/** Choosing `rating` for a take that has `current`: a second choice of the same one clears it. */
export function toggledRating(current: TakeRating | undefined, rating: TakeRating): TakeRating {
  return (current ?? "none") === rating ? "none" : rating;
}

/** Whether Delete Bad Takes has anything to delete (a folder keeps at least one take). */
export function hasBadTakes(f: TakeFolder): boolean {
  return f.takes.length > 1 && f.takes.some((t) => t.rating === "bad");
}
