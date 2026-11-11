// Multi-output instruments, kept pure so it can be tested: which output a strip's "+" takes next,
// what an aux on an instrument's output shows as its input, and whether a bounce has such auxes to
// take along (see `session/mixer/multi_out/`).
import type { Id, Track } from "../engine/project";
import type { InstrumentOutputs, OutputBusInfo } from "../engine/snapshot";

type Aux = Pick<Track, "id" | "instrument_output">;

/** The instrument's next output with no aux on it, or null (one that has none left, or no extras). */
export function nextOutput(all: readonly InstrumentOutputs[] | undefined, tracks: readonly Aux[], track: Id): OutputBusInfo | null {
  const outputs = all?.find((o) => o.track === track)?.outputs ?? [];
  const used = (bus: number) => tracks.some((t) => t.instrument_output?.track === track && t.instrument_output.bus === bus);
  return outputs.find((b) => b.index > 0 && !used(b.index)) ?? null;
}

/** Whether the instrument on this track has outputs past its main one. */
export function hasExtraOutputs(all: readonly InstrumentOutputs[] | undefined, track: Id): boolean {
  return !!all?.some((o) => o.track === track && o.outputs.length > 1);
}

export interface AuxInput {
  label: string;
  title: string;
  /** The output isn't there: the aux is silent. */
  silent: boolean;
}

/** What an aux shows in its input slot. Its bus can still be sent to, whatever feeds it. */
export function auxInput(
  aux: Pick<Track, "bus" | "instrument_output" | "frozen_output">,
  tracks: readonly Pick<Track, "id" | "name">[],
  all: readonly InstrumentOutputs[] | undefined,
): AuxInput {
  const from = aux.instrument_output;
  if (!from) return { label: `Bus ${aux.bus}`, title: "Sends and outputs set to this bus play through this strip", silent: false };
  const name = tracks.find((t) => t.id === from.track)?.name ?? "?";
  // A frozen instrument's plug-in is unloaded, so its outputs aren't listed: the aux plays the render.
  if (aux.frozen_output) {
    return {
      label: `${name}: output ${from.bus + 1}, frozen`,
      title: `${name} is frozen: this strip plays the render of its output ${from.bus + 1}, as does anything sent to Bus ${aux.bus}. Unfreeze ${name} to play it live`,
      silent: false,
    };
  }
  const bus = all?.find((o) => o.track === from.track)?.outputs.find((b) => b.index === from.bus);
  if (!bus) {
    return {
      label: `${name}: no output ${from.bus + 1}`,
      title: `${name}'s instrument has no output ${from.bus + 1} now (it was replaced, or the plug-in is missing), so this aux gets nothing from it. It still plays what's sent to Bus ${aux.bus}`,
      silent: true,
    };
  }
  return { label: `${name}: ${bus.name}`, title: `This output of ${name}'s instrument plays through this strip, as does anything sent to Bus ${aux.bus}`, silent: false };
}

/** Whether any of `sources` (tracks a bounce renders) has auxes on its instrument's outputs. */
export function hasOutputAuxes(tracks: readonly Pick<Track, "instrument_output">[], sources: readonly Id[]): boolean {
  return tracks.some((t) => !!t.instrument_output && sources.includes(t.instrument_output.track));
}
