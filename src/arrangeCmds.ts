// Logic's arranging commands in the Tracks area: Edit › Select, Section Between Locators, Trim
// and Move, Paste Replace, Paste at Original Position, Join per Track, Delete and Shuffle, Strip
// Silence, and naming and colouring regions and tracks after each other. They are shared by
// the native Edit menu (menu.rs ids), the Tracks area's Edit menu and the key commands
// (keymap.ts, same ids). The session makes the edits
// (crates/engine/src/session/arrange_cmds.rs); the selection criteria are in regionSelect.ts.
import { act, actMade, call, type Id, type Region, type Track } from "./api";
import type { MenuItem } from "./components/Menu";
import { openStripSilence } from "./components/StripSilence";
import { keyHint } from "./keymap";
import { regionEnd } from "./loops";
import { menuLabel, type MenuId } from "./menuLabels";
import { joinRegions } from "./ops";
import * as crit from "./regionSelect";
import { currentTick, getState, setState } from "./store";

const project = () => getState().snap?.project;
const tracks = (): Track[] => project()?.tracks ?? [];
const playhead = () => currentTick()?.pos ?? 0;
const sel = () => getState().selectedRegions;
const toast = (text: string) => setState({ toast: { text, at: Date.now(), ms: 2500 } });
const select = (ids: Id[] | null) => ids && setState({ selectedRegions: ids, focus: "regions" });

/** The tracks in view, as Select All sees them (hidden ones only in hide mode). */
function visible(): Track[] {
  const showHidden = getState().showHidden;
  return tracks().filter((t) => !t.hidden || showHidden);
}

const regionsOf = (ids: Id[]): Region[] => tracks().flatMap((t) => t.regions.filter((r) => ids.includes(r.id)));

/** The marquee, if there is one with a length. */
function marquee() {
  const m = getState().marquee;
  return m && m.end > m.start && m.tracks.length ? m : null;
}

/** The locators (the cycle range), or null with a hint when they're not set. */
function locators(): { start: number; end: number } | null {
  const c = project()?.cycle;
  if (c && c.end > c.start) return { start: c.start, end: c.end };
  toast("Set the locators first (drag in the ruler)");
  return null;
}

/** What the section and crop commands act on: the marquee (its tracks only) or the locators
 *  (every track). */
function sectionRange(): { start: number; end: number; tracks: Id[] | null } | null {
  const m = marquee();
  if (m) return { start: m.start, end: m.end, tracks: m.tracks };
  const l = locators();
  return l && { ...l, tracks: null };
}

// ---- Edit › Select ------------------------------------------------------------------------

const selectFollowing = (track?: Id) => select(crit.following(visible(), crit.followingFrom(visible(), sel(), playhead()), track));

function selectFollowingOnTrack() {
  // The track of the first selected region, or the selected track.
  const first = regionsOf(sel()).sort((a, b) => a.start - b.start)[0];
  const t = first ? tracks().find((x) => x.regions.some((r) => r.id === first.id))?.id : getState().snap?.selected;
  if (t != null) selectFollowing(t);
}

function selectInsideLocators() {
  const l = locators();
  if (l) select(crit.insideLocators(visible(), l.start, l.end));
}

// ---- Section Between Locators ---------------------------------------------------------------

/** Insert Silence, Cut Section (Snip) or Repeat Section, between the locators across every
 *  track, or within the marquee on its tracks. A cut section also goes on the region
 *  clipboard, as in Logic. */
async function sectionEdit(edit: "insert_silence" | "cut" | "repeat") {
  const r = sectionRange();
  if (!r) return;
  if (edit === "cut") {
    const items = await call<[number, Region][]>("marquee_copy", { tracks: r.tracks ?? tracks().map((t) => t.id), start: r.start, end: r.end });
    if (items?.length) setState({ clipboard: items.map(([index, region]) => ({ index, region })), clipboardStart: r.start });
  }
  const made = await actMade("section_edit", { edit, start: r.start, end: r.end, tracks: r.tracks });
  if (made) {
    if (r.tracks) setState({ marquee: null });
    if (made.length) select(made);
  }
}

/** Insert Cut Section (Splice): the section cut last goes in at the left locator (or the
 *  marquee's start), moving what follows along. */
async function insertCutSection() {
  const m = marquee();
  const at = m?.start ?? project()?.cycle.start ?? 0;
  select(await actMade("insert_cut_section", { at }));
}

// ---- Trim and Move --------------------------------------------------------------------------

/** Runs a command on the selected regions (nothing when none is selected). */
function onSelected(cmd: string, args: Record<string, unknown> = {}) {
  const ids = sel();
  if (ids.length) act(cmd, { ids, ...args });
}

function fillWithinLocators() {
  const l = locators();
  if (!l) return;
  // The selection, or every region starting between the locators.
  const ids = sel().length ? sel() : visible().flatMap((t) => t.regions.filter((r) => r.start >= l.start && r.start < l.end).map((r) => r.id));
  if (ids.length) act("fill_within_locators", { ids, start: l.start, end: l.end });
}

/** Crop Regions Outside Locators/Marquee: the regions under the marquee are cut to it, or the
 *  selected ones to the locators. */
async function cropRegions() {
  const m = marquee();
  if (m) {
    const ids = tracks()
      .filter((t) => m.tracks.includes(t.id))
      .flatMap((t) => t.regions.filter((r) => r.start < m.end && regionEnd(r) > m.start).map((r) => r.id));
    if (!ids.length) return;
    setState({ marquee: null });
    select(await actMade("crop_regions", { ids, start: m.start, end: m.end }));
    return;
  }
  const l = sel().length ? locators() : null;
  if (l) select(await actMade("crop_regions", { ids: sel(), start: l.start, end: l.end }));
}

const trimToPlayhead = (end: boolean) => onSelected("trim_regions_to", { pos: Math.round(playhead()), end });

// ---- Paste ----------------------------------------------------------------------------------

/** The clipboard's regions, each with the track it lands on when the top one goes on `top`
 *  (keeping their spacing across tracks), and the clipboard's start. */
function clipboardItems(top: number): { items: [Id, Region][]; earliest: number } | null {
  const s = getState();
  const clip = s.clipboard;
  const all = tracks();
  if (!clip.length || !all.length) return null;
  const first = Math.min(...clip.map((c) => c.index));
  const items = clip.flatMap(({ index, region }) => {
    const t = all[top + index - first];
    return t ? [[t.id, region] as [Id, Region]] : [];
  });
  return { items, earliest: s.clipboardStart ?? Math.min(...clip.map((c) => c.region.start)) };
}

/** Paste at Original Position: back where the regions were copied from, on their own tracks. */
async function pasteAtOriginal() {
  const clip = getState().clipboard;
  const got = clipboardItems(Math.min(...clip.map((c) => c.index)));
  if (got) select(await actMade("paste_regions", { items: got.items, offset: 0, label: "Paste at Original Position" }));
}

/** Paste Replace: pastes as Paste does (at the playhead on the selected track, or at the
 *  marquee), replacing what lies under the pasted range on the tracks it lands on. */
async function pasteReplace() {
  const all = tracks();
  const m = marquee();
  const targetId = m?.tracks[0] ?? getState().snap?.selected ?? all[0]?.id;
  const got = clipboardItems(Math.max(0, all.findIndex((t) => t.id === targetId)));
  if (!got) return;
  const made = await actMade("paste_replace", { items: got.items, offset: (m?.start ?? playhead()) - got.earliest });
  if (made && made.length < getState().clipboard.length) toast("Some regions didn't fit the track they'd land on");
  if (m) setState({ marquee: null });
  select(made);
}

function stripSilence() {
  const ids = regionsOf(sel()).filter((r) => r.content.type === "audio" && !r.takes).map((r) => r.id);
  if (ids.length) openStripSilence(ids);
  else toast("Select audio regions first");
}

// ---- commands, menus ------------------------------------------------------------------------

/** Every command by its menu and key command id. */
export const ARRANGE_COMMANDS: Record<string, () => void> = {
  "sel-following": () => selectFollowing(),
  "sel-following-track": selectFollowingOnTrack,
  "sel-inside-locators": selectInsideLocators,
  "sel-muted": () => select(crit.muted(visible())),
  "sel-overlapped": () => select(crit.overlapped(visible())),
  "sel-same-color": () => select(crit.sameColor(visible(), sel())),
  "sel-equal": () => select(crit.matching(visible(), sel(), crit.sameContent)),
  "sel-similar": () => select(crit.matching(visible(), sel(), crit.similarContent)),
  "sel-invert": () => select(crit.invert(visible(), sel())),
  "section-insert-silence": () => sectionEdit("insert_silence"),
  "section-cut": () => sectionEdit("cut"),
  "section-repeat": () => sectionEdit("repeat"),
  "section-insert-cut": insertCutSection,
  "trim-start-playhead": () => trimToPlayhead(false),
  "trim-end-playhead": () => trimToPlayhead(true),
  "trim-end-next": () => onSelected("trim_to_next_region"),
  "fill-locators": fillWithinLocators,
  "remove-overlaps": () => onSelected("remove_overlaps"),
  "same-length": () => onSelected("resize_to_same_length"),
  "crop-outside": cropRegions,
  "remove-gaps": () => onSelected("remove_gaps"),
  "move-to-playhead": () => onSelected("move_regions_to", { pos: Math.round(playhead()) }),
  "paste-original": pasteAtOriginal,
  "paste-replace": pasteReplace,
  "delete-shuffle": () => {
    const ids = sel();
    if (ids.length) act("delete_and_shuffle", { ids }).then(() => setState({ selectedRegions: [] }));
  },
  "join-per-track": () => joinRegions(),
  "strip-silence": stripSilence,
  "name-regions-by-track": () => onSelected("name_regions_by_track"),
  "name-tracks-by-region": () => onSelected("name_tracks_by_region"),
  "color-regions-by-track": () => onSelected("color_regions_by_track"),
  "color-tracks-by-region": () => onSelected("color_tracks_by_region"),
};

/** Runs a native menu item of these commands; false when `id` is another menu's. */
export function runArrangeCommand(id: string): boolean {
  const run = ARRANGE_COMMANDS[id];
  run?.();
  return !!run;
}

/** A command's menu entry: its name (or `label`), its key and its action. */
const item = (id: MenuId, disabled = false, label: string = menuLabel(id)): MenuItem => ({ id, label, hint: keyHint(id), disabled, onClick: ARRANGE_COMMANDS[id] });

/** The entries of these commands in the Edit menus (src/menuLabels.ts EDIT_MENU lays them out),
 *  with their names and when they're greyed out. Section and Crop name the marquee when there
 *  is one, as they then act on it. */
export function arrangeItems(): MenuItem[] {
  const none = !sel().length;
  const m = !!marquee();
  const where = m ? "Marquee" : "Locators";
  const empty = !getState().clipboard.length;
  return [
    item("paste-original", empty),
    item("paste-replace", empty),
    item("join-per-track", sel().length < 2),
    item("strip-silence", none),
    item("sel-following"),
    item("sel-following-track"),
    item("sel-inside-locators"),
    item("sel-muted"),
    item("sel-overlapped"),
    item("sel-same-color", none),
    item("sel-equal", none),
    item("sel-similar", none),
    item("sel-invert"),
    item("section-insert-silence", false, `Insert Silence Between ${where}`),
    item("section-cut", false, `Cut Section Between ${where}`),
    item("section-repeat", false, `Repeat Section Between ${where}`),
    item("section-insert-cut", false, `Insert Cut Section at ${m ? "Marquee" : "Left Locator"}`),
    item("trim-start-playhead", none),
    item("trim-end-playhead", none),
    item("trim-end-next", none),
    item("fill-locators"),
    item("remove-overlaps", none),
    item("same-length", sel().length < 2),
    item("crop-outside", none && !m, `Crop Regions Outside ${where}`),
    item("move-to-playhead", none),
    item("remove-gaps", none),
    item("delete-shuffle", none),
    item("name-regions-by-track", none),
    item("name-tracks-by-region", none),
    item("color-regions-by-track", none),
    item("color-tracks-by-region", none),
  ];
}
