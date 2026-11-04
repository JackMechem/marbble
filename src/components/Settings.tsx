// The Settings window (Marbble › Settings…, ⌘,), Project Settings (File › Project
// Settings…, ⌥P) and the crash-recovery sheet. Modelled on Logic Pro's Settings: a row of
// icon tabs, each pane a form whose changes apply at once.
//
// The panes are in settingsPanes/, one file each; this file is the window that holds them.
import { Icon } from "../icons";
import { setState, useStore, type SettingsTab } from "../store";
import { EditingPane } from "./settingsPanes/EditingPane";
import { GeneralPane } from "./settingsPanes/GeneralPane";
import { KeysPane } from "./settingsPanes/KeysPane";
import { MetronomePane } from "./settingsPanes/MetronomePane";
import { MidiPane } from "./settingsPanes/MidiPane";
import { SheetFrame, TabIcon } from "./settingsPanes/parts";
import { PluginsPane } from "./settingsPanes/PluginsPane";
import { RecordingPane } from "./settingsPanes/RecordingPane";

// App.tsx and DetachedWindow.tsx import these from here.
export { ProjectSettingsSheet } from "./settingsPanes/ProjectSettings";
export { RecoverySheet } from "./settingsPanes/RecoverySheet";

type PaneProps = { close: () => void };

/**
 * Marbble › Settings. The Audio and Appearance panes live in Dialogs.tsx and are passed in.
 */
export function SettingsWindow({ audio: AudioPane, appearance: AppearancePane }: { audio: React.FC<PaneProps>; appearance: React.FC<PaneProps> }) {
  const tab = useStore((s) => s.settingsTab);
  const close = () => setState({ dialog: null });
  const tabs: { id: SettingsTab; label: string; icon: React.ReactNode; pane: React.FC<PaneProps> }[] = [
    { id: "general", label: "General", icon: <TabIcon.general />, pane: GeneralPane },
    { id: "audio", label: "Audio", icon: <Icon.waveform />, pane: AudioPane },
    { id: "recording", label: "Recording", icon: <TabIcon.recording />, pane: RecordingPane },
    { id: "metronome", label: "Metronome", icon: <TabIcon.metronome />, pane: MetronomePane },
    { id: "midi", label: "MIDI", icon: <TabIcon.midi />, pane: MidiPane },
    { id: "editing", label: "Editing", icon: <TabIcon.editing />, pane: EditingPane },
    { id: "plugins", label: "Plug-ins", icon: <TabIcon.plugins />, pane: PluginsPane },
    { id: "keys", label: "Key Commands", icon: <Icon.keyboard />, pane: KeysPane },
    { id: "appearance", label: "Appearance", icon: <Icon.palette />, pane: AppearancePane },
  ];
  const current = tabs.find((t) => t.id === tab) ?? tabs[1];
  const Pane = current.pane;
  return (
    <SheetFrame title={`Settings — ${current.label}`} onClose={close} width={760}>
      <div className="settings-tabs wide">
        {tabs.map((t) => (
          <button key={t.id} className={"settings-tab" + (current.id === t.id ? " on" : "")} onClick={() => setState({ settingsTab: t.id })}>
            {t.icon}
            <span>{t.label}</span>
          </button>
        ))}
      </div>
      <div className="settings-body">
        <Pane close={close} />
      </div>
    </SheetFrame>
  );
}
