//! What an assignment drives: a mixer control, a plug-in parameter, a transport button or a key
//! command ([`CtlTarget`]), or whichever parameter is selected ([`SelectedParam`]).

use serde::{Deserialize, Serialize};

use crate::model::Id;

/// A transport button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportAction {
    /// Play / Stop.
    Play,
    Record,
    /// Cycle on or off.
    Cycle,
}

/// A band setting of the fixed Channel EQ (see `CtlTarget::Eq`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqField {
    On,
    Freq,
    Gain,
    Q,
}

/// What a controller drives. Tracks and plug-ins are referred to by their ids in the project,
/// so an assignment whose track or plug-in isn't in the open project does nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CtlTarget {
    Volume {
        track: Id,
    },
    Pan {
        track: Id,
    },
    Mute {
        track: Id,
    },
    Solo {
        track: Id,
    },
    /// Record enable.
    Arm {
        track: Id,
    },
    Send {
        track: Id,
        slot: u8,
    },
    /// A built-in effect's or a VST3 plug-in's parameter (`track` is MASTER_ID for Stereo
    /// Out). `plugin` is the slot's plug-in id: in another project the same instance id may
    /// hold a different plug-in, and then the assignment doesn't apply.
    Plugin {
        track: Id,
        uid: Id,
        plugin: String,
        param: u32,
    },
    /// A band setting of the fixed Channel EQ strips had before it became an insert (`track` is
    /// MASTER_ID for Stereo Out). Kept so assignments learnt then still work: it drives that
    /// setting of the strip's first Channel EQ insert. New assignments name the insert's
    /// parameter (`Plugin`).
    Eq {
        track: Id,
        band: u8,
        field: EqField,
    },
    /// The fixed Channel EQ's output gain (see `Eq`).
    EqOutput {
        track: Id,
    },
    /// Stereo Out's fader.
    MasterVolume,
    Transport {
        action: TransportAction,
    },
    /// A key command (an id of src/keyboard/keymap.ts's KEY_COMMANDS, or of a menu item with a key):
    /// the session passes it to the main window, which runs it as if its key was pressed.
    Command {
        id: String,
    },
    /// Input monitoring.
    Monitor {
        track: Id,
    },
    /// An Audio FX slot's bypass switch (on is bypassed).
    Bypass {
        track: Id,
        slot: u8,
    },
    /// A VCA fader's level.
    VcaLevel {
        vca: Id,
    },
    /// A VCA's mute.
    VcaMute {
        vca: Id,
    },
    /// A control of the selected track, whichever it is when the controller moves (Logic's
    /// Selected Track targets). With no track selected it does nothing.
    Selected {
        param: SelectedParam,
    },
}

/// A control of the selected track (see `CtlTarget::Selected`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SelectedParam {
    Volume,
    Pan,
    Mute,
    Solo,
    Arm,
    Monitor,
    Send {
        slot: u8,
    },
    /// An Audio FX slot's bypass.
    Bypass {
        slot: u8,
    },
    /// The focused plug-in's parameter at this position in its list: the plug-in whose window
    /// or panel was last opened or touched on that track, else its instrument, else its first
    /// Audio FX.
    PluginParam {
        index: u16,
    },
}

impl SelectedParam {
    /// The target this is on `track`; a plug-in parameter needs the plug-in, so it's None.
    pub fn on(self, track: Id) -> Option<CtlTarget> {
        Some(match self {
            SelectedParam::Volume => CtlTarget::Volume { track },
            SelectedParam::Pan => CtlTarget::Pan { track },
            SelectedParam::Mute => CtlTarget::Mute { track },
            SelectedParam::Solo => CtlTarget::Solo { track },
            SelectedParam::Arm => CtlTarget::Arm { track },
            SelectedParam::Monitor => CtlTarget::Monitor { track },
            SelectedParam::Send { slot } => CtlTarget::Send { track, slot },
            SelectedParam::Bypass { slot } => CtlTarget::Bypass { track, slot },
            SelectedParam::PluginParam { .. } => return None,
        })
    }

    /// Its name after "Selected Track › ".
    pub fn name(self) -> String {
        match self {
            SelectedParam::Volume => "Volume".into(),
            SelectedParam::Pan => "Pan".into(),
            SelectedParam::Mute => "Mute".into(),
            SelectedParam::Solo => "Solo".into(),
            SelectedParam::Arm => "Record Enable".into(),
            SelectedParam::Monitor => "Input Monitoring".into(),
            SelectedParam::Send { slot } => format!("Send {}", slot + 1),
            SelectedParam::Bypass { slot } => format!("Insert {} Bypass", slot + 1),
            SelectedParam::PluginParam { index } => format!("Plug-in Parameter {}", index + 1),
        }
    }
}

impl CtlTarget {
    /// Switches act on a press (a value above 63 after one at or below it) and ignore the
    /// release, so momentary buttons toggle them, as in Logic.
    pub fn is_button(&self) -> bool {
        matches!(
            self,
            CtlTarget::Mute { .. }
                | CtlTarget::Solo { .. }
                | CtlTarget::Arm { .. }
                | CtlTarget::Transport { .. }
                | CtlTarget::Eq { field: EqField::On, .. }
                | CtlTarget::Command { .. }
                | CtlTarget::Monitor { .. }
                | CtlTarget::Bypass { .. }
                | CtlTarget::VcaMute { .. }
                | CtlTarget::Selected {
                    param: SelectedParam::Mute | SelectedParam::Solo | SelectedParam::Arm | SelectedParam::Monitor | SelectedParam::Bypass { .. }
                }
        )
    }

    /// The track (or MASTER_ID) the target belongs to, if any.
    pub fn track(&self) -> Option<Id> {
        match self {
            CtlTarget::Volume { track }
            | CtlTarget::Pan { track }
            | CtlTarget::Mute { track }
            | CtlTarget::Solo { track }
            | CtlTarget::Arm { track }
            | CtlTarget::Send { track, .. }
            | CtlTarget::Plugin { track, .. }
            | CtlTarget::Eq { track, .. }
            | CtlTarget::EqOutput { track }
            | CtlTarget::Monitor { track }
            | CtlTarget::Bypass { track, .. } => Some(*track),
            CtlTarget::MasterVolume => Some(crate::model::MASTER_ID),
            CtlTarget::Transport { .. } | CtlTarget::Command { .. } | CtlTarget::VcaLevel { .. } | CtlTarget::VcaMute { .. } | CtlTarget::Selected { .. } => {
                None
            }
        }
    }
}
