// The Tracks area's right-click menus: on a region, on an empty spot of a lane and on a track
// header, and the colour menu they share. The operations they run are in trackCommands.ts and
// regionCommands.ts.
import { openFxPanel } from "../plugins/fxHost";
import { canRecord, isStack, type Track } from "../engine/project";
import { type Region } from "../engine/regions";
import { act, actMade, api } from "../engine/commands";
import { newAux } from "../mixer/routing";
import type { MenuItem } from "../ui/Menu";
import { askName, notEmpty } from "../ui/NameSheet";
import { openDrumReplace } from "./drumReplace/DrumReplaceSheet";
import { trackIconMenu } from "./icons/TrackIconPicker";
import { setTrackHeight } from "./trackLayout";
import { isLooped } from "./regions/loops";
import { BOUNCE_IN_PLACE_HINT, openBounce, openBounceInPlace } from "../project/bounce/bounce";
import { fmt, keyHint } from "../keyboard/keymap";
import { menuLabel, SHORT_LABELS } from "../app/menuBar/menuLabels";
import { exportMidi, exportTrackMidi } from "../project/midiFiles";
import { canFreeze, freezeTracks, unfreezeTracks } from "./freeze";
import { getState, setState } from "../state/store";
import { tempoMap } from "../lib/tempo";
import { TRACK_COLOR_NAMES, TRACK_COLORS } from "./trackColors";
import { isFolder, packTakeFolder } from "./takes/takes";
import { takeFolderMenu } from "./takes/takeFolderMenu";
import { followingRegion, shapeOf } from "./regions/fades";
import { getSettings } from "../settings/appSettings";
import { canHaveAlternatives, trackAltMenu } from "./alternatives/trackAlternatives";
import { isDefault, paramsOf } from "./regions/regionParams";
import { selectedRegions, trackOf, cutRegions, copyRegions, pasteRegions, repeatRegions, splitAtPlayhead, joinRegions, toggleLoop, convertLoops, toggleRegionMute, setLocatorsByRegions, selectOnTrack, selectAllRegions, deleteRegions, createPatternRegion, regionById, importMidiAt, importAt, createMidiRegion, selectRegions } from "./regionCommands";
import { selectedTracks, duplicateTracks, bounceInPlace, createStack, toggleCollapsed, flattenStack, removeFromStack, groupTracks, setGroup, updateGroup, switchAll, setHidden, hideTracks, unhideAllTracks, toggleShowHidden, deleteTracks, deleteUnusedTracks } from "./trackCommands";

const project = () => getState().snap?.project;

// ---- menus ----------------------------------------------------------------------------------

const ms = (p: { sample_rate: number }, v: number) => Math.round((v / 1000) * p.sample_rate);

/** A colour menu: a five-by-four grid of the track colours, after a "default" item when
 *  `defaultLabel` is given. */
export function colorMenu(current: string | null | undefined, set: (c: string | null) => void, defaultLabel?: string): MenuItem[] {
  const grid: MenuItem = { swatches: TRACK_COLORS.map((c, i) => ({ color: c, name: TRACK_COLOR_NAMES[i], checked: c === current, onClick: () => set(c) })) };
  if (defaultLabel) return [{ label: defaultLabel, checked: !current, onClick: () => set(null) }, { separator: true }, grid];
  return [grid];
}

/** Rename… in a region's menu. */
export async function renameRegion(r: Region) {
  const name = await askName({ title: "Rename Region", value: r.name, ok: "Rename", check: notEmpty });
  if (name && name !== r.name) act("rename_region", { id: r.id, name });
}

/** The menu for right-clicking a region (acting on the whole selection). */
export function regionMenu(r: Region, rename: () => void): MenuItem[] {
  const p = project()!;
  const ids = selectedRegions().includes(r.id) ? selectedRegions() : [r.id];
  const regs = ids.map(regionById).filter((x): x is Region => !!x);
  const audio = regs.filter((x) => x.content.type === "audio").map((x) => x.id);
  const midi = regs.filter((x) => x.content.type === "midi").map((x) => x.id);
  // A beat and a bar at the tempo where the (first) region starts.
  const beat = tempoMap(p).samplesPerBeatAt(r.start);
  const bar = tempoMap(p).samplesPerBarAt(r.start);
  const fades = (which: "fade_in" | "fade_out"): MenuItem[] =>
    (
      [
        ["None", 0],
        ["10 ms", ms(p, 10)],
        ["50 ms", ms(p, 50)],
        ["250 ms", ms(p, 250)],
        ["1 Beat", Math.round(beat)],
        ["1 Bar", Math.round(bar)],
      ] as [string, number][]
    ).map(([label, v]) => ({ label, onClick: () => act("set_region_fades", { ids: audio, fadeIn: which === "fade_in" ? v : null, fadeOut: which === "fade_out" ? v : null }) }));
  // Crossfade with the following region (Logic's EqP by default, or the region's own type).
  const meets = regs.filter((x) => x.content.type === "audio" && followingRegion(trackOf(x.id)?.regions ?? [], x)).map((x) => x.id);
  // The first entry follows Settings › Editing › Crossfade Time (the engine reads it).
  const xfadeMs = getSettings().prefs.editing.crossfade_ms ?? 20;
  const crossfades = (
    [
      [`Crossfade Time (${xfadeMs} ms)`, null],
      ["None", 0],
      ["10 ms", ms(p, 10)],
      ["20 ms", ms(p, 20)],
      ["50 ms", ms(p, 50)],
      ["250 ms", ms(p, 250)],
      ["1 Beat", Math.round(beat)],
    ] as [string, number | null][]
  ).map(([label, v]): MenuItem => ({ label, onClick: () => act("crossfade_regions", { ids: meets, length: v }) }));
  const muted = regs.every((x) => x.muted);
  const looped = regs.filter(isLooped);
  const loopedAudio = looped.filter((x) => x.content.type === "audio" && !x.takes);
  const items: MenuItem[] = [
    { label: menuLabel("cut"), hint: keyHint("cut"), onClick: () => cutRegions(ids) },
    { label: menuLabel("copy"), hint: keyHint("copy"), onClick: () => copyRegions(ids) },
    { label: menuLabel("paste"), hint: keyHint("paste"), disabled: !getState().clipboard.length, onClick: () => pasteRegions() },
    { label: menuLabel("duplicate"), hint: fmt("Mod+D"), onClick: () => repeatRegions(1, ids) },
    {
      label: "Repeat",
      children: [2, 3, 4, 8, 16].map((n) => ({ label: `${n} Times`, onClick: () => repeatRegions(n, ids) })),
    },
    { separator: true },
    { label: SHORT_LABELS.split, hint: fmt("Mod+T"), onClick: () => splitAtPlayhead(ids) },
    { label: SHORT_LABELS.join, hint: fmt("Mod+J"), disabled: ids.length < 2, onClick: () => joinRegions(ids) },
    { label: SHORT_LABELS.loop, hint: keyHint("loop"), checked: looped.length === regs.length, onClick: () => toggleLoop(ids) },
    { label: "Convert Loops to Real Copies", disabled: !looped.length, onClick: () => convertLoops(ids) },
    // Writes the Region inspector's playback parameters into the regions (Logic's Normalize).
    { label: "Apply Region Parameters", disabled: regs.every((x) => isDefault(paramsOf(x))), onClick: () => act("apply_region_params", { ids }) },
    { label: muted ? "Unmute" : "Mute", hint: keyHint("mute-regions"), onClick: () => toggleRegionMute(ids) },
    { label: "Rename…", disabled: ids.length > 1, onClick: rename },
    { label: "Color", children: colorMenu(regs.length === 1 ? r.color : undefined, (c) => act("set_region_color", { ids, color: c }), "Track Color") },
  ];
  if (audio.length) {
    items.push(
      { separator: true },
      {
        label: "Gain",
        children: [
          ...([6, 3, 1, -1, -3, -6] as number[]).map((d) => ({ label: `${d > 0 ? "+" : ""}${d} dB`, onClick: () => act("set_region_gain", { ids: audio, delta: d, db: null }) })),
          { separator: true },
          { label: "Reset to 0 dB", onClick: () => act("set_region_gain", { ids: audio, db: 0, delta: null }) },
          { label: "Normalize", onClick: () => act("normalize_regions", { ids: audio }) },
        ],
      },
      { label: "Fade In", children: fades("fade_in") },
      { label: "Fade Out", children: fades("fade_out") },
      { label: "Crossfade", disabled: !meets.length, children: crossfades },
      ...(loopedAudio.length
        ? [{ label: "Crossfade Loop Repetitions", checked: loopedAudio.every((x) => shapeOf(x.content).loop_xfade), onClick: () => act("set_loop_crossfade", { ids: loopedAudio.map((x) => x.id), on: !loopedAudio.every((x) => shapeOf(x.content).loop_xfade) }) }]
        : []),
      { label: "Reverse", onClick: () => act("reverse_regions", { ids: audio }) },
    );
    // Take folders: the folder's own menu, and Pack Take Folder for plain regions (on one
    // track or several).
    const packable = audio.length > 1 && regs.every((x) => !x.takes);
    items.push(
      { separator: true },
      ...(isFolder(r) ? [{ label: "Take Folder", children: takeFolderMenu(r) }] : []),
      { label: menuLabel("pack-takes"), hint: keyHint("pack-takes"), disabled: !packable, onClick: () => packTakeFolder(audio) },
    );
  }
  if (midi.length) {
    items.push(
      { separator: true },
      {
        label: "Quantize",
        children: [4, 8, 16, 32].map((d) => ({ label: `1/${d} Note`, onClick: () => act("quantize_regions", { ids: midi, grid: Math.round((p.sample_rate * 60 * 4) / p.tempo / d) }) })),
      },
      {
        label: "Transpose",
        children: (
          [
            ["Octave Up", 12],
            ["Semitone Up", 1],
            ["Semitone Down", -1],
            ["Octave Down", -12],
          ] as [string, number][]
        ).map(([label, n]) => ({ label, onClick: () => act("transpose_regions", { ids: midi, semitones: n }) })),
      },
      { label: "Export as MIDI File…", onClick: () => exportMidi(midi, midi.length === 1 ? regs.find((x) => x.id === midi[0])!.name : p.name) },
      // A MIDI take folder (MIDI cycle recording with "Create take folders").
      ...(isFolder(r) && r.content.type === "midi" ? [{ label: "Take Folder", children: takeFolderMenu(r) }] : []),
    );
    // Pattern regions (Step Sequencer) and back.
    const patterns = regs.filter((x) => x.pattern).map((x) => x.id);
    const plain = regs.filter((x) => x.content.type === "midi" && !x.pattern).map((x) => x.id);
    items.push(
      patterns.length
        ? { label: "Convert to MIDI Region", onClick: () => act("convert_patterns_to_midi", { ids: patterns }) }
        : { label: "Convert to Pattern Region", onClick: () => act("convert_midi_to_patterns", { ids: plain }) },
    );
  }
  items.push(
    { separator: true },
    { label: "Bounce in Place…", onClick: () => openBounceInPlace([], ids) },
    { label: "Export as Audio File…", disabled: ids.length > 1, onClick: () => openBounce("region", r.id) },
  );
  if (ids.length === 1 && r.content.type === "audio") {
    const convert = (slices: number | null) =>
      actMade("convert_region_to_sampler", { region: r.id, slices }).then((made) => {
        if (!made?.length) return;
        api.selectTrack(made[0]);
        const uid = getState().snap?.project.tracks.find((t) => t.id === made[0])?.instrument?.uid;
        if (uid !== undefined) openFxPanel(uid);
      });
    items.push({
      label: "Convert to Sampler Track",
      children: [
        { label: "Whole Region, One Key Plays It", onClick: () => convert(null) },
        { label: "Slice at Transients, One Key Each", onClick: () => convert(0.5) },
      ],
    });
  }
  const track = trackOf(r.id);
  items.push(
    { separator: true },
    { label: "Move to New Track", onClick: () => actMade("regions_to_new_track", { ids }).then(selectRegions) },
    { label: SHORT_LABELS["set-locators"], hint: keyHint("set-locators"), onClick: () => setLocatorsByRegions(ids) },
    {
      label: "Select",
      children: [
        { label: "All on Track", disabled: !track, onClick: () => track && selectOnTrack(track.id) },
        { label: "All Following on Track", disabled: !track, onClick: () => track && selectOnTrack(track.id, r.start) },
        { label: "All", hint: fmt("Mod+A"), onClick: selectAllRegions },
      ],
    },
    { separator: true },
    { label: menuLabel("delete"), hint: keyHint("delete-regions"), onClick: () => deleteRegions(ids) },
  );
  return items;
}

/** Right-clicking empty space in a track's lane (or below the tracks). */
export function laneMenu(track: Track | null, pos: number): MenuItem[] {
  const items: MenuItem[] = [];
  if (track) items.push({ label: "Paste Here", hint: keyHint("paste"), disabled: !getState().clipboard.length, onClick: () => pasteRegions({ track: track.id, pos }) });
  if (track?.kind === "instrument") items.push({ label: "Create MIDI Region", onClick: () => createMidiRegion(track.id, pos) });
  if (track?.kind === "instrument") items.push({ label: "Create Pattern Region", onClick: () => createPatternRegion(track.id, pos) });
  if (track?.kind === "audio") items.push({ label: "Import Audio File Here…", onClick: () => importAt(track, pos) });
  if (track?.kind === "instrument") items.push({ label: "Import MIDI File Here…", onClick: () => importMidiAt(track, pos) });
  if (track) items.push({ label: "Select All on Track", onClick: () => selectOnTrack(track.id) });
  if (items.length) items.push({ separator: true });
  items.push({ label: "Select All", hint: fmt("Mod+A"), onClick: selectAllRegions }, { label: "New Tracks…", hint: fmt("Mod+Alt+N"), onClick: () => setState({ dialog: "new-tracks" }) });
  return items;
}

/** Right-clicking a track header (acting on the selected tracks). */
export function trackMenu(t: Track, rename: () => void): MenuItem[] {
  const p = project()!;
  const ids = selectedTracks().includes(t.id) ? selectedTracks() : [t.id];
  const many = ids.length > 1;
  const sel = p.tracks.filter((x) => ids.includes(x.id));
  const group = t.group != null ? p.groups.find((g) => g.id === t.group) : undefined;
  const inStack = sel.some((x) => x.parent !== null);
  const s = (one: string, more: string) => (many ? more : one);
  const items: MenuItem[] = [
    { label: s("Duplicate Track", "Duplicate Tracks"), hint: fmt("Mod+D"), onClick: () => duplicateTracks(true) },
    { label: "New Track with Same Settings", disabled: many, onClick: () => duplicateTracks(false) },
    { label: "New Tracks…", hint: fmt("Mod+Alt+N"), onClick: () => setState({ dialog: "new-tracks" }) },
    { label: "New Aux", onClick: () => newAux() },
    { label: s("Bounce in Place…", "Bounce Tracks in Place…"), hint: BOUNCE_IN_PLACE_HINT, onClick: () => bounceInPlace(ids) },
    ...(sel.some(canFreeze)
      ? [
          sel.filter(canFreeze).every((x) => x.frozen)
            ? { label: s("Unfreeze Track", "Unfreeze Tracks"), onClick: () => unfreezeTracks(ids) }
            : { label: s("Freeze Track", "Freeze Tracks"), disabled: !sel.some((x) => canFreeze(x) && x.regions.length), onClick: () => freezeTracks(ids) },
        ]
      : []),
    ...(t.kind === "audio" && !many ? [{ label: "Drum Replacement/Doubling…", disabled: !t.regions.length, onClick: () => openDrumReplace(t.id) }] : []),
    { separator: true },
    { label: "Rename Track", disabled: many, onClick: rename },
    { label: "Track Color", children: colorMenu(many ? undefined : t.color, (c) => c && sel.forEach((x) => api.updateTrack(x.id, { color: c }))) },
    { label: "Track Icon", children: trackIconMenu(t) },
    ...(canHaveAlternatives(t) && !many ? [{ label: "Track Alternatives", children: trackAltMenu(t) }] : []),
    { separator: true },
    {
      label: "Create Track Stack",
      children: [
        { label: "Folder Stack", hint: fmt("Mod+Shift+D"), onClick: () => createStack(false, ids) },
        { label: "Summing Stack", onClick: () => createStack(true, ids) },
      ],
    },
  ];
  if (isStack(t.kind) && !many) {
    items.push(
      { label: t.collapsed ? "Expand Stack" : "Collapse Stack", onClick: () => toggleCollapsed(t) },
      { label: "Flatten Stack", onClick: () => flattenStack(t.id) },
    );
  }
  if (inStack) items.push({ label: "Remove from Stack", onClick: () => removeFromStack(ids) });
  items.push(
    { separator: true },
    {
      label: "Group",
      children: [
        { label: s("New Group from Track", "New Group from Selected Tracks"), hint: fmt("Mod+G"), onClick: () => groupTracks(ids) },
        ...(p.groups.length ? [{ separator: true } as MenuItem, { label: "Assign to", header: true } as MenuItem] : []),
        ...p.groups.map((g) => ({ label: g.name, checked: sel.every((x) => x.group === g.id), onClick: () => setGroup(ids, g.id) })),
        { separator: true },
        { label: "Remove from Group", disabled: !sel.some((x) => x.group !== null), onClick: () => setGroup(ids, null) },
        { label: "Group Settings…", disabled: !group, onClick: () => group && setState({ groupSheet: group.id }) },
        { label: group?.enabled === false ? "Turn Group On" : "Turn Group Off", disabled: !group, onClick: () => group && updateGroup({ ...group, enabled: !group.enabled }) },
      ],
    },
    { separator: true },
    { label: many ? "Tracks On" : "Track On", hint: keyHint("track-on-off"), checked: sel.every((x) => !x.off), onClick: () => switchAll("off", ids) },
    { label: many ? "Mute Tracks" : "Mute", hint: keyHint("track-mute"), checked: sel.every((x) => x.mute), onClick: () => switchAll("mute", ids) },
    { label: many ? "Solo Tracks" : "Solo", hint: keyHint("track-solo"), checked: sel.every((x) => x.solo), onClick: () => switchAll("solo", ids) },
  );
  if (sel.some((x) => canRecord(x.kind))) items.push({ label: "Record Enable", hint: keyHint("track-arm"), checked: sel.every((x) => x.armed || !canRecord(x.kind)), onClick: () => switchAll("armed", ids.filter((id) => canRecord(p.tracks.find((x) => x.id === id)!.kind))) });
  items.push(
    { separator: true },
    { label: "Select All Regions on Track", disabled: many, onClick: () => selectOnTrack(t.id) },
    ...(t.kind === "instrument" && !many ? [{ label: "Export Track as MIDI File…", disabled: !t.regions.length, onClick: () => exportTrackMidi(t.id) }] : []),
    { label: "Reset Track Height", onClick: () => ids.forEach((id) => setTrackHeight(id, null)) },
    { separator: true },
    // One key for both: hideTracks (⌃H) shows the tracks again when they're all hidden.
    sel.every((x) => x.hidden)
      ? { label: s("Unhide Track", "Unhide Tracks"), hint: keyHint("hide-tracks"), onClick: () => setHidden(ids, false) }
      : { label: s("Hide Track", "Hide Tracks"), hint: keyHint("hide-tracks"), onClick: () => hideTracks(ids) },
    { label: "Unhide All Tracks", disabled: !p.tracks.some((x) => x.hidden), onClick: unhideAllTracks },
    { label: "Show Hidden Tracks", hint: keyHint("show-hidden"), checked: getState().showHidden, onClick: toggleShowHidden },
    { separator: true },
    { label: s("Delete Track", "Delete Tracks"), hint: fmt("Mod+Backspace"), onClick: () => deleteTracks(ids) },
    { label: "Delete Unused Tracks", onClick: deleteUnusedTracks },
  );
  return items;
}
