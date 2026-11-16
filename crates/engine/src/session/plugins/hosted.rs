//! Plug-ins from other makers, in either format the engine hosts: VST3 (`vst3-host`) and CLAP
//! (`clap-host`). Both give the same two halves (`plugin_api`), so past loading the session and
//! the engine treat them alike.

use plugin_api::{Controller, PluginDescriptor, PluginFormat, Processor};

/// A loaded plug-in's two halves: the processor for the engine, the controller for the session.
pub type Halves = (Box<dyn Processor>, Box<dyn Controller>);

/// Loads the plug-in `desc` describes, with its side-chain input active (`keyed`) where it has
/// one. Main thread.
pub fn instantiate(desc: &PluginDescriptor, sample_rate: f64, max_block: usize, keyed: bool) -> Result<Halves, String> {
    match desc.format {
        PluginFormat::Vst3 => {
            let (p, c) = vst3_host::instantiate_keyed(desc, sample_rate, max_block, keyed)?;
            Ok((Box::new(p), Box::new(c)))
        }
        PluginFormat::Clap => {
            let (p, c) = clap_host::instantiate(desc, sample_rate, max_block)?;
            Ok((Box::new(p), Box::new(c)))
        }
    }
}
