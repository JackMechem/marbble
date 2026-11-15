// A plug-in's format as the lists show it. VST3 is the usual one, so only CLAP plug-ins are
// tagged in the slot menus and the Library; the Plug-in Manager names every plug-in's. Pure, so
// the tests can run it.
import type { PluginDesc } from "../engine/listings";

type HasFormat = Pick<PluginDesc, "format">;

/** "VST3" or "CLAP". A plug-in listed before formats were told apart is a VST3. */
export function formatName(p: HasFormat): string {
  return p.format === "clap" ? "CLAP" : "VST3";
}

/** The tag beside a plug-in's name in the slot menus and the Library, or none for a VST3. */
export function formatTag(p: HasFormat): string | undefined {
  return p.format === "clap" ? "CLAP" : undefined;
}

/** The heading third-party plug-ins come under in the slot menus: the formats among them. */
export function formatsHeading(list: readonly HasFormat[]): string {
  const there = new Set(list.map(formatName));
  return ["VST3", "CLAP"].filter((f) => there.has(f)).join(" & ") || "VST3";
}
