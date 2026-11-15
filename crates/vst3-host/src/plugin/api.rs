//! The two halves behind the engine's plug-in interface (`plugin_api`): each call goes to the
//! method of the same name.

use super::*;

impl plugin_api::Processor for PluginProcessor {
    fn name(&self) -> &str {
        PluginProcessor::name(self)
    }
    fn max_block(&self) -> usize {
        PluginProcessor::max_block(self)
    }
    fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], key: KeyInput, midi: &[MidiEvent], t: &TransportInfo) {
        PluginProcessor::process_keyed(self, left, right, key, midi, t)
    }
    fn queue_param(&mut self, id: u32, offset: i32, value: f64) {
        PluginProcessor::queue_param(self, id, offset, value)
    }
    fn suspend(&mut self) {
        PluginProcessor::suspend(self)
    }
    fn close_gate(&mut self) {
        PluginProcessor::close_gate(self)
    }
    fn shut_gate(&mut self) {
        PluginProcessor::shut_gate(self)
    }
    fn open_gate(&mut self, settle: u32) {
        PluginProcessor::open_gate(self, settle)
    }
    fn has_edits(&self) -> bool {
        PluginProcessor::has_edits(self)
    }
    fn wants_to_run(&self) -> bool {
        PluginProcessor::wants_to_run(self)
    }
    fn ring_out(&self) -> u32 {
        PluginProcessor::ring_out(self)
    }
    fn pass_edits(&mut self, t: &TransportInfo) {
        PluginProcessor::pass_edits(self, t)
    }
    fn output_bus(&self, bus: usize, n: usize) -> Option<(&[f32], &[f32])> {
        PluginProcessor::output_bus(self, bus, n)
    }
}

impl plugin_api::Controller for PluginController {
    fn desc(&self) -> &PluginDescriptor {
        &self.desc
    }
    fn set_unresponsive(&mut self, on: bool) {
        PluginController::set_unresponsive(self, on)
    }
    fn instance(&self) -> u64 {
        PluginController::instance(self)
    }
    fn responsive(&self) -> bool {
        PluginController::responsive(self)
    }
    fn has_editor(&self) -> bool {
        PluginController::has_editor(self)
    }
    fn editor_open(&self) -> bool {
        PluginController::editor_open(self)
    }
    fn open_editor(&mut self, title: &str) -> Result<(), String> {
        PluginController::open_editor(self, title)
    }
    fn close_editor(&mut self) {
        PluginController::close_editor(self)
    }
    fn idle(&mut self) {
        PluginController::idle(self)
    }
    fn latency(&self) -> u32 {
        PluginController::latency(self)
    }
    fn take_latency_changed(&mut self) -> bool {
        PluginController::take_latency_changed(self)
    }
    fn has_key_input(&self) -> bool {
        PluginController::has_key_input(self)
    }
    fn key_active(&self) -> bool {
        PluginController::key_active(self)
    }
    fn parked(&self) -> bool {
        PluginController::parked(self)
    }
    fn set_key_active(&mut self, on: bool) -> Result<(), String> {
        PluginController::set_key_active(self, on)
    }
    fn audio_buses(&self) -> Vec<BusLayout> {
        PluginController::audio_buses(self)
    }
    fn output_buses(&self) -> &[OutputBus] {
        PluginController::output_buses(self)
    }
    fn set_active_outputs(&mut self, want: &[u32]) {
        PluginController::set_active_outputs(self, want)
    }
    fn parameters(&self) -> Vec<ParamInfo> {
        PluginController::parameters(self)
    }
    fn param_names(&self) -> Vec<(u32, String, bool)> {
        PluginController::param_names(self)
    }
    fn param_value(&self, id: u32) -> Option<f64> {
        PluginController::param_value(self, id)
    }
    fn set_param_value(&self, id: u32, value: f64) {
        PluginController::set_param_value(self, id, value)
    }
    fn edit_param(&self, id: u32, value: f64) {
        PluginController::edit_param(self, id, value)
    }
    fn host_changes(&self) -> u64 {
        PluginController::host_changes(self)
    }
    fn host_echo(&self, id: u32, value: f64) -> bool {
        PluginController::host_echo(self, id, value)
    }
    fn edits_pending(&self) -> bool {
        PluginController::edits_pending(self)
    }
    fn take_params_restarted(&mut self) -> bool {
        PluginController::take_params_restarted(self)
    }
    fn take_gestures(&mut self) -> Vec<Gesture> {
        PluginController::take_gestures(self)
    }
    fn drive_handler(&self, g: Gesture) {
        PluginController::drive_handler(self, g)
    }
    fn drive_params_restarted(&self) {
        PluginController::drive_params_restarted(self)
    }
    fn save_state(&self) -> Option<PluginState> {
        PluginController::save_state(self)
    }
    fn load_state(&self, st: &PluginState) {
        PluginController::load_state(self, st)
    }
}
