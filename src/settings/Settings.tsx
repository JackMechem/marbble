// The Settings window (Marbble › Settings…, ⌘,): a row of icon tabs, each pane a form whose changes
// apply at once (Audio's wait for Apply). The panes are in panes/, one file each; this file is the
// window that holds them. settings/ also holds the app-wide settings themselves (appSettings.ts)
// and the words the panes show (settingsText.ts).
import { useEffect } from "react";
import { Icon } from "../ui/icons";
import { AppearancePane } from "./panes/AppearancePane";
import { AudioPane, forgetAudioDraft } from "./panes/AudioPane";
import { setState, useStore, type SettingsTab } from "../state/store";
import { EditingPane } from "./panes/EditingPane";
import { GeneralPane } from "./panes/GeneralPane";
import { KeysPane } from "./panes/KeysPane";
import { MidiPane } from "./panes/MidiPane";
import { SheetFrame, TabIcon } from "./panes/parts";
import { PluginsPane } from "./panes/PluginsPane";
import { RecordingPane } from "./panes/RecordingPane";

type PaneProps = { close: () => void };

/** Marbble › Settings. */
export function SettingsDialog() {
  // Unapplied audio settings last as long as the window.
  useEffect(
    () => () => {
      forgetAudioDraft();
    },
    [],
  );
  return <SettingsWindow />;
}

function SettingsWindow() {
  const tab = useStore((s) => s.settingsTab);
  const close = () => setState({ dialog: null });
  const tabs: { id: SettingsTab; label: string; icon: React.ReactNode; pane: React.FC<PaneProps> }[] = [
    { id: "general", label: "General", icon: <TabIcon.general />, pane: GeneralPane },
    { id: "audio", label: "Audio", icon: <Icon.waveform />, pane: AudioPane },
    { id: "recording", label: "Recording", icon: <TabIcon.recording />, pane: RecordingPane },
    { id: "midi", label: "MIDI", icon: <TabIcon.midi />, pane: MidiPane },
    { id: "editing", label: "Editing", icon: <TabIcon.editing />, pane: EditingPane },
    { id: "plugins", label: "Plug-ins", icon: <TabIcon.plugins />, pane: PluginsPane },
    { id: "keys", label: "Key Commands", icon: <Icon.keyboard />, pane: KeysPane },
    { id: "appearance", label: "Appearance", icon: <Icon.palette />, pane: AppearancePane },
  ];
  const current = tabs.find((t) => t.id === tab) ?? tabs[1];
  const Pane = current.pane;
  return (
    <SheetFrame title={`Settings — ${current.label}`} onClose={close} width={760} className="settings-sheet">
      <div className="settings-tabs wide">
        {tabs.map((t) => (
          <button key={t.id} className={"settings-tab" + (current.id === t.id ? " on" : "")} aria-pressed={current.id === t.id} onClick={() => setState({ settingsTab: t.id })}>
            {t.icon}
            <span>{t.label}</span>
          </button>
        ))}
      </div>
      {/* Keyed by pane, so each opens at its top rather than where the last one was scrolled. */}
      <div className="settings-body" key={current.id}>
        <Pane close={close} />
      </div>
    </SheetFrame>
  );
}
