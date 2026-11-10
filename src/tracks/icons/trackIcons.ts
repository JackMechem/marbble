// Track icons (Logic's Track Icon), kept pure so they can be tested: the icon set by group, and the
// icon a track shows. A track shows the icon the user chose (`Track::icon`), else one its name
// suggests ("Kick In" → kick), else its kind's. The suggestion is worked out each time, not
// saved, so it follows the name until an icon is chosen. The drawings are in trackIconArt.tsx.
import type { Track, TrackKind } from "../../engine/project";

export interface TrackIconInfo {
  id: string;
  /** For tooltips and screen readers. */
  name: string;
}

export interface TrackIconGroup {
  label: string;
  icons: TrackIconInfo[];
}

const group = (label: string, icons: [string, string][]): TrackIconGroup => ({ label, icons: icons.map(([id, name]) => ({ id, name })) });

/** The icons in the picker's order. The first group is the kinds' own icons. */
export const TRACK_ICON_GROUPS: TrackIconGroup[] = [
  group("Tracks", [
    ["audio", "Audio"],
    ["instrument", "Instrument"],
    ["aux", "Aux"],
    ["folder", "Folder Stack"],
    ["summing", "Summing Stack"],
  ]),
  group("Drums", [
    ["kick", "Kick"],
    ["snare", "Snare"],
    ["hihat", "Hi-Hat"],
    ["toms", "Toms"],
    ["overheads", "Overheads"],
    ["room", "Room"],
    ["kit", "Drum Kit"],
    ["drum-machine", "Drum Machine"],
  ]),
  group("Percussion", [
    ["congas", "Congas"],
    ["shaker", "Shaker"],
    ["tambourine", "Tambourine"],
    ["cowbell", "Cowbell"],
  ]),
  group("Bass", [
    ["bass", "Bass Guitar"],
    ["upright-bass", "Upright Bass"],
  ]),
  group("Guitars", [
    ["electric-guitar", "Electric Guitar"],
    ["acoustic-guitar", "Acoustic Guitar"],
    ["amp", "Amp"],
  ]),
  group("Keys", [
    ["piano", "Piano"],
    ["organ", "Organ"],
    ["synth", "Synth"],
  ]),
  group("Strings", [
    ["violin", "Violin"],
    ["harp", "Harp"],
  ]),
  group("Brass", [
    ["trumpet", "Trumpet"],
    ["trombone", "Trombone"],
  ]),
  group("Woodwinds", [
    ["sax", "Saxophone"],
    ["flute", "Flute"],
  ]),
  group("Vocals", [
    ["mic", "Vocal"],
    ["choir", "Choir"],
  ]),
  group("FX and Buses", [
    ["fx", "Effects"],
    ["reverb", "Reverb"],
    ["delay", "Delay"],
    ["headphones", "Cue Mix"],
    ["speaker", "Output"],
  ]),
];

const BY_ID = new Map(TRACK_ICON_GROUPS.flatMap((g) => g.icons.map((i) => [i.id, i] as const)));

/** The icon with this id, if the set has it (a project from a newer version may name others). */
export const trackIconInfo = (id: string | null | undefined): TrackIconInfo | undefined => (id ? BY_ID.get(id) : undefined);

/**
 * Words in a track's name and the icon they suggest. The first entry with a word in the name
 * wins, so specific ones ("bass drum", "amp") come before general ones ("bass", "guitar"). A
 * word also matches with an "s" or a number on the end ("Toms", "Kick2"). Add to it freely.
 */
export const NAME_ICONS: [string[], string][] = [
  [["kick", "kik", "bd", "bass drum"], "kick"],
  [["snare", "snr", "sd"], "snare"],
  [["hat", "hh", "hihat", "hi hat"], "hihat"],
  [["tom"], "toms"],
  [["oh", "overhead", "cymbal", "ride", "crash"], "overheads"],
  [["room", "rm", "ambience", "amb"], "room"],
  [["drum machine", "808", "909", "beat"], "drum-machine"],
  [["drum", "kit"], "kit"],
  [["conga", "bongo", "perc", "percussion"], "congas"],
  [["shaker"], "shaker"],
  [["tamb", "tambourine"], "tambourine"],
  [["cowbell"], "cowbell"],
  [["upright", "double bass", "contrabass"], "upright-bass"],
  [["amp"], "amp"],
  [["bass"], "bass"],
  [["acoustic", "ac gtr", "nylon"], "acoustic-guitar"],
  [["gtr", "guitar"], "electric-guitar"],
  [["piano", "pno", "keys", "rhodes", "wurli"], "piano"],
  [["organ", "b3", "hammond"], "organ"],
  [["synth", "pad", "arp"], "synth"],
  [["violin", "viola", "cello", "string", "fiddle"], "violin"],
  [["harp"], "harp"],
  [["trumpet", "tpt", "brass", "horn"], "trumpet"],
  [["trombone", "tbn"], "trombone"],
  [["sax", "saxophone"], "sax"],
  [["flute", "clarinet", "woodwind", "oboe"], "flute"],
  [["choir", "bv", "bgv", "backing vocal", "backing vox", "harmony"], "choir"],
  [["vox", "vocal", "voc", "voice", "vo", "singer"], "mic"],
  [["reverb", "verb"], "reverb"],
  [["delay", "echo"], "delay"],
  [["fx", "effect"], "fx"],
  [["cue", "headphone", "phones"], "headphones"],
];

/** A name as lower-case words with a space round each: "KickIn_2" → " kick in 2 ". */
function words(name: string): string {
  return ` ${name.replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase().replace(/[^a-z0-9]+/g, " ").trim()} `;
}

const PATTERNS: [RegExp[], string][] = NAME_ICONS.map(([names, id]) => [names.map((n) => new RegExp(` ${n}s?\\d* `)), id]);

/** The icon a track's name suggests, if any. */
export function iconFromName(name: string): string | undefined {
  const w = words(name);
  return PATTERNS.find(([res]) => res.some((re) => re.test(w)))?.[1];
}

/** A kind's own icon. */
const kindIcon = (kind: TrackKind): string => kind;

/** The icon a track shows when none is chosen: its name's, else its kind's. */
export const defaultIcon = (t: Pick<Track, "kind" | "name">): string => iconFromName(t.name) ?? kindIcon(t.kind);

/** What the Track inspector calls a track's icon: the chosen one's name, else Default. */
export const iconLabel = (t: Pick<Track, "icon">): string => trackIconInfo(t.icon)?.name ?? "Default";

/** The icon a track shows. */
export const shownIcon = (t: Pick<Track, "kind" | "name" | "icon">): string => (trackIconInfo(t.icon) ? t.icon! : defaultIcon(t));

/**
 * Where an arrow key (or Home or End) moves the picker's focus, in rows of `lengths` buttons
 * (Default's row, then a row per group). Left and Right go on to the row before or after; Up and
 * Down keep the column as far as the row allows. Other keys, and moves off the ends, stay put.
 */
export function pickerMove(lengths: number[], row: number, col: number, key: string): [number, number] {
  const last = lengths.length - 1;
  switch (key) {
    case "ArrowLeft":
      if (col > 0) return [row, col - 1];
      return row > 0 ? [row - 1, lengths[row - 1] - 1] : [row, col];
    case "ArrowRight":
      if (col < lengths[row] - 1) return [row, col + 1];
      return row < last ? [row + 1, 0] : [row, col];
    case "ArrowUp":
      return row > 0 ? [row - 1, Math.min(col, lengths[row - 1] - 1)] : [row, col];
    case "ArrowDown":
      return row < last ? [row + 1, Math.min(col, lengths[row + 1] - 1)] : [row, col];
    case "Home":
      return [0, 0];
    case "End":
      return [last, lengths[last] - 1];
    default:
      return [row, col];
  }
}
