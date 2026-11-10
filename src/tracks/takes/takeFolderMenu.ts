// A take folder's menu (its name bar and its take rows): the comps, the takes with their ratings
// and colours, the comp section under the pointer, Flatten, Unpack and the rest.
import type { Id } from "../../engine/project";
import type { Region, Take, TakeFolder, TakeRating } from "../../engine/regions";
import { act, actMade } from "../../engine/commands";
import { compNames, currentComp, isMidiFolder, neighbourTake, sectionAt } from "./comps";
import type { MenuItem } from "../../ui/Menu";
import { askName, notEmpty } from "../../ui/NameSheet";
import { colorMenu } from "../contextMenus";
import { setState } from "../../state/store";
import { hasBadTakes, RATING_MARKS, takeLabel, toggledRating } from "./takeRatings";
import { takeShift, wholeTake, isOpen, toggleFolder } from "./takes";

/** The comp section part of a take row's menu: for the section of `take` heard at timeline `at`. */
function sectionItems(r: Region & { takes: TakeFolder }, take: Id, at: number): MenuItem[] {
  const s = sectionAt(r.takes, at + takeShift(r));
  if (!s || s.take !== take) return [];
  const move = (delta: number) => ({
    disabled: neighbourTake(r.takes, s, delta) === null,
    onClick: () => {
      setState({ compSection: { region: r.id, at } });
      act("move_comp_section", { region: r.id, at, delta });
    },
  });
  return [
    { label: "Comp Section", header: true },
    { label: "Move Section to Take Above", ...move(-1) },
    { label: "Move Section to Take Below", ...move(1) },
    { label: "Remove Section", onClick: () => act("remove_comp_section", { region: r.id, at }) },
    { separator: true },
  ];
}

async function renameTake(r: Region, t: Take) {
  const name = await askName({ title: "Rename Take", value: t.name, ok: "Rename", check: notEmpty });
  if (name && name !== t.name) act("rename_take", { region: r.id, take: t.id, name });
}

async function renameComp(r: Region & { takes: TakeFolder }, index: number) {
  const old = compNames(r.takes)[index];
  const name = await askName({ title: "Rename Comp", value: old, ok: "Rename", check: notEmpty });
  if (name && name !== old) act("rename_comp", { region: r.id, index, name });
}

/** The comps part of an audio folder's menu, as in Logic: the comps to switch between, then
 *  Create New Comp, Duplicate Comp, Rename Comp and Delete Comp. */
function compItems(r: Region & { takes: TakeFolder }): MenuItem[] {
  const names = compNames(r.takes);
  const current = currentComp(r.takes);
  return [
    { label: "Comps", header: true },
    ...names.map((name, index) => ({ label: name, checked: index === current, onClick: () => act("select_comp", { region: r.id, index }) })),
    { separator: true },
    { label: "Create New Comp", onClick: () => act("create_comp", { region: r.id }) },
    { label: "Duplicate Comp", onClick: () => act("duplicate_comp", { region: r.id }) },
    { label: "Rename Comp…", onClick: () => renameComp(r, current) },
    { label: "Delete Comp", disabled: names.length < 2, onClick: () => act("delete_comp", { region: r.id, index: current }) },
    { separator: true },
  ];
}

/** Good and Bad for one take, ticked when it has them; choosing the ticked one clears it. */
function ratingItems(r: Region, t: Take): MenuItem[] {
  const rate = (rating: TakeRating) => act("set_take_rating", { region: r.id, take: t.id, rating: toggledRating(t.rating, rating) });
  return [
    { label: `${RATING_MARKS.good} Good`, checked: t.rating === "good", onClick: () => rate("good") },
    { label: `${RATING_MARKS.bad} Bad`, checked: t.rating === "bad", onClick: () => rate("bad") },
  ];
}

/** A take's colour menu: the folder's colour, or one of its own. */
function takeColorItems(r: Region, t: Take): MenuItem[] {
  return colorMenu(t.color, (color) => act("set_take_color", { region: r.id, take: t.id, color }), "Folder Color");
}

/** The take folder menu (the badge, or Take Folder in the region menu). A MIDI folder plays
 *  whole takes, so it has no comps, and Flatten and Merge is just Flatten. Right-clicking a take
 *  row (`row`: its take and the timeline position) puts that take's rating and colour first,
 *  then the edits of the comp section there. */
export function takeFolderMenu(r: Region & { takes: TakeFolder }, row?: { take: Id; at: number }): MenuItem[] {
  const f = r.takes;
  const midi = isMidiFolder(r);
  const whole = midi ? (f.comp[0]?.take ?? f.takes[f.takes.length - 1]?.id) : wholeTake(f);
  const select = (ids: Id[] | null) => ids && setState({ selectedRegions: ids, focus: "regions" });
  const active = whole === null ? "Comp" : "Take";
  const rowTake = row && f.takes.find((t) => t.id === row.take);
  const own: MenuItem[] = rowTake
    ? [{ label: takeLabel(rowTake), header: true }, ...ratingItems(r, rowTake), { label: "Color", children: takeColorItems(r, rowTake) }, { separator: true }]
    : [];
  return [
    ...own,
    ...(row && !midi ? sectionItems(r, row.take, row.at) : []),
    ...(midi ? [] : compItems(r)),
    { label: "Takes", header: true },
    ...f.takes.map((t, k) => ({ label: `${k + 1}  ${takeLabel(t)}`, checked: whole === t.id, onClick: () => act("select_take", { region: r.id, take: t.id }) })),
    { separator: true },
    { label: isOpen(r.id) ? "Close Take Folder" : "Open Take Folder", onClick: () => toggleFolder(r.id) },
    { label: "Rename Take", children: f.takes.map((t) => ({ label: t.name, onClick: () => renameTake(r, t) })) },
    { label: "Rate Take", children: f.takes.map((t) => ({ label: takeLabel(t), children: ratingItems(r, t) })) },
    { label: "Take Color", children: f.takes.map((t) => ({ label: takeLabel(t), children: takeColorItems(r, t) })) },
    { label: "Delete Take", disabled: f.takes.length < 2, children: f.takes.map((t) => ({ label: t.name, onClick: () => act("delete_take", { region: r.id, take: t.id }) })) },
    { label: "Delete Unused Takes", disabled: f.comp.length === 0 || new Set(f.comp.map((s) => s.take)).size === f.takes.length, onClick: () => act("delete_unused_takes", { region: r.id }) },
    { label: "Delete Bad Takes", disabled: !hasBadTakes(f), onClick: () => act("delete_bad_takes", { region: r.id }) },
    { separator: true },
    { label: "Flatten", onClick: () => actMade("flatten_take_folder", { region: r.id }).then(select) },
    ...(midi ? [] : [{ label: "Flatten and Merge", onClick: () => actMade("merge_take_folder", { region: r.id }).then(select) }]),
    { label: "Unpack to New Tracks", onClick: () => actMade("unpack_take_folder", { region: r.id }) },
    { label: "Unpack to Existing Tracks", onClick: () => actMade("unpack_to_existing_tracks", { region: r.id }) },
    { separator: true },
    // As in Logic, the items name the comp when one is heard rather than a whole take.
    { label: `Export Active ${active} to New Track`, onClick: () => actMade("export_active_take", { region: r.id, remove: false }) },
    { label: `Move Active ${active} to New Track`, onClick: () => actMade("export_active_take", { region: r.id, remove: true }) },
  ];
}
