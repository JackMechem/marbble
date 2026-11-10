// The Event List's working parts the rest builds on: the working list of events, the shared
// selection, sending edits, scrolling a row into view and the sound of a pressed note. The
// command, gesture and key modules beside this one take its result, and EventList.tsx puts them
// together on each render, as the Piano Roll does.
import { invoke } from "@tauri-apps/api/core";
import { type Project } from "../../engine/project";
import { type Region } from "../../engine/regions";
import { act } from "../../engine/commands";
import { getState, setState, type MidiSelection } from "../../state/store";
import { barLines, tempoMap } from "../../lib/tempo";
import * as me from "../midiEdit";
import * as el from "./model";
import { prefs } from "./prefs";
import { scopeKey, type Scope } from "./scope";

/** Row height in CSS pixels (the rows are laid out at multiples of it). */
export const ROW_H = 18;

export const EMPTY_LISTS: el.Lists = { notes: [], cc: [] };

export interface Work {
  /** The track's regions this was built from (identity), the listed region ids, and the
   *  selection it was built with. */
  source: Region[];
  key: string;
  sel: MidiSelection | null;
  /** The listed regions as they are after our last edit (ahead of the snapshot until it comes). */
  regions: Region[];
  lists: el.Lists;
}

/** A value being typed in: the row's key, its field and the text it started with. */
export interface Editing {
  key: string;
  field: el.Field;
  text: string;
}

/** What's live for the handlers, which run after the render that made them; refreshed on each
 *  render. */
export interface Live {
  project: Project | undefined;
  scope: Scope | null;
  rows: el.Row[];
}

type El<T> = React.RefObject<T | null>;

/** The component's refs and callbacks, which every part of the list works through. The parts hold
 *  no state of their own, so building them anew on each render can't lose anything. */
export interface Refs {
  work: React.RefObject<Work>;
  /** Edits sent but not yet answered: until the last answer is in, snapshots are older than the
   *  working list, so it isn't rebuilt from them. */
  inflight: React.RefObject<number>;
  /** What a drag in progress makes the list, shown in its place. The rows keep their order until
   *  it is sent. */
  preview: React.RefObject<el.Lists | null>;
  /** The row a Shift-click selects from. */
  anchor: React.RefObject<number | null>;
  /** Scroll the first selected row into view after the next render. */
  reveal: React.RefObject<boolean>;
  /** The cell being typed in; the ref stops Enter and the blur that follows both committing. */
  editing: React.RefObject<Editing | null>;
  setEditing: (e: Editing | null) => void;
  /** The note playing for a press or a drag. */
  sounding: React.RefObject<number | null>;
  live: React.RefObject<Live>;
  rootRef: El<HTMLDivElement>;
  scrollRef: El<HTMLDivElement>;
  bump: () => void;
}

export type Core = ReturnType<typeof listCore>;

export function listCore(refs: Refs) {
  const { work, inflight, preview, reveal, sounding, live, rootRef, scrollRef, bump } = refs;

  // ---- the working list -------------------------------------------------------------------

  /** Builds the working list afresh from `sc`'s regions, with the shared selection `sel`. */
  const rebuild = (sc: Scope | null, sel: MidiSelection) => {
    const lists = sc ? me.fromRegions(sc.regions, new Set(sel.notes), new Set(sel.cc)) : EMPTY_LISTS;
    work.current = { source: sc?.track.regions ?? [], key: scopeKey(sc), sel, regions: sc?.regions ?? [], lists };
  };

  /** Whether the working list is out of date: the snapshot, the scope or the shared selection
   *  changed, and no edit is waiting for its answer. */
  const isStale = (sc: Scope | null, sel: MidiSelection) => {
    const w = work.current;
    return inflight.current === 0 && (w.source !== (sc?.track.regions ?? w.source) || w.key !== scopeKey(sc) || w.sel !== sel);
  };

  /** Shares the selection: this list's keys replace those of the listed regions. */
  const publish = (lists: el.Lists) => {
    const ids = new Set(work.current.regions.map((r) => r.id));
    const keep = (k: string) => !ids.has(Number(k.split(":")[0]));
    const prev = getState().midiSelection;
    const sel = { notes: [...prev.notes.filter(keep), ...me.selectionKeys(lists.notes)], cc: [...prev.cc.filter(keep), ...me.selectionKeys(lists.cc)] };
    work.current = { ...work.current, sel };
    setState({ midiSelection: sel });
  };

  const setSelection = (lists: el.Lists) => {
    work.current = { ...work.current, lists };
    publish(lists);
    bump();
  };

  /** Sends edited lists as one undo step (`key` joins it to the step with that key). */
  const commit = (lists: el.Lists, label: string, key?: string) => {
    preview.current = null;
    const pr = live.current.project;
    if (!pr) return;
    const { edits, selNotes, selCc } = me.toEdits(lists.notes, lists.cc, work.current.regions, barLines(tempoMap(pr)));
    if (!edits.length) {
      setSelection(lists);
      return;
    }
    const next = me.applyEdits(work.current.regions, edits);
    const w = me.fromRegions(next, selNotes, selCc);
    work.current = { ...work.current, regions: next, lists: w };
    publish(w);
    reveal.current = true;
    bump();
    inflight.current++;
    act("set_midi_content", { edits, label, key: key ?? null }).finally(() => {
      if (--inflight.current === 0) {
        // Take the snapshot (and anything it refused) from here on.
        work.current = { ...work.current, source: [] };
        bump();
      }
    });
  };

  // ---- view -------------------------------------------------------------------------------

  const focusList = () => rootRef.current?.focus({ preventScroll: true });

  /** Scrolls row `i` into view, a third of the way down, if it's out of view. */
  const scrollToRow = (i: number) => {
    const box = scrollRef.current;
    if (!box || i < 0) return;
    const top = i * ROW_H;
    if (top < box.scrollTop || top + ROW_H > box.scrollTop + box.clientHeight) box.scrollTop = Math.max(0, top - box.clientHeight / 3);
  };

  // ---- sound ------------------------------------------------------------------------------

  /** Stops the note sounding and, with MIDI Out on, plays `pitch` (null just stops). */
  const sound = (pitch: number | null, velocity = 100) => {
    if (sounding.current !== null) invoke("midi_event", { data: [0x80, sounding.current, 0] }).catch(() => {});
    sounding.current = null;
    const t = live.current.scope?.track;
    if (pitch === null || !prefs.midiOut || !t) return;
    // Live MIDI plays on the selected track.
    if (getState().snap?.selected !== t.id) act("select_track", { id: t.id });
    sounding.current = pitch;
    invoke("midi_event", { data: [0x90, pitch, Math.max(1, Math.min(127, velocity))] }).catch(() => {});
  };

  return { ...refs, rebuild, isStale, setSelection, commit, focusList, scrollToRow, sound };
}
