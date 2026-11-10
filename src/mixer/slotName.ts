// What a plug-in slot calls its plug-in. Slots are narrow, so a built-in instrument shows its
// model ("Tre" rather than "Marbble Tre", which would read "Marbble ..." in every strip); the
// slot's tooltip keeps the whole name. Pure, so it can be unit tested.

/** The brand in front of built-in instruments' names; projects from before the rename say Fermata. */
const BRAND = /^(Marbble|Fermata)\s+/;

/** The short name for a slot: a built-in plug-in's name without the brand, anything else as it is. */
export function slotName(pluginId: string, name: string): string {
  if (!pluginId.startsWith("builtin:")) return name;
  const short = name.replace(BRAND, "");
  return short || name;
}
