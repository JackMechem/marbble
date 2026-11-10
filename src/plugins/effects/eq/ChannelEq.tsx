// The Channel EQ's panel (crates/engine/src/fx/channel_eq/): the editor (EqEditor.tsx) working on the
// insert's parameters, with the analyzer showing what comes out of it. Bypass and presets are in
// the panel's header, and a bypassed one dims (fx-linear-phase.css), as for any built-in effect.
import { useContext } from "react";
import type { ChannelEq } from "../../../engine/project";
import type { FxPanelProps } from "../../builtinEffects";
import { FxLearnContext, learnContext } from "../../../controllers/learn";
import { changedParams, eqFromValues, OUTPUT, paramId } from "./eqParams";
import { useStore } from "../../../state/store";
import { EqEditor, type EqControl } from "./EqEditor";

const idOf = (c: EqControl) => (c.band === null ? OUTPUT : paramId(c.band, c.field));

export function ChannelEqPanel(p: FxPanelProps) {
  const sr = useStore((s) => s.snap?.project.sample_rate ?? 48000);
  const fx = useContext(FxLearnContext);
  const eq = eqFromValues(p.values);

  // An edit can move several parameters at once (a point dragged in the graph moves frequency
  // and gain): they go together, as one undo step.
  const apply = (next: ChannelEq) => {
    const changes = changedParams(eq, next);
    if (changes.length === 1 || !p.setMany) changes.forEach(([id, v]) => p.set(id, v));
    else if (changes.length > 1) p.setMany(changes);
  };
  const learn = (id: number) => (fx ? learnContext({ type: "plugin", track: fx.track, uid: fx.uid, plugin: "", param: id }) : undefined);

  return (
    <EqEditor
      eq={eq}
      sr={sr}
      onChange={apply}
      analyzerUid={p.uid}
      learn={(c) => learn(idOf(c))}
      onGrab={(controls, on) => controls.forEach((c) => p.touch(idOf(c), on))}
    />
  );
}
