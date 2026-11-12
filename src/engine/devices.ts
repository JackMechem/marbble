// Audio devices and the audio configuration (Settings › Audio), as the engine reports them, and the
// system mixer's input level the launch check warns about.

export interface DeviceInfo {
  id: string;
  name: string;
  channels: number;
  sample_rates: number[];
  is_default: boolean;
}

export interface HostInfo {
  name: string;
  devices: number;
}

export interface DeviceList {
  host: string;
  hosts: HostInfo[];
  inputs: DeviceInfo[];
  outputs: DeviceInfo[];
}

/** The sample rates `cfg`'s devices both support (the output's, less any the input lacks). */
export function supportedRates(cfg: AudioConfig, devices: DeviceList | null): number[] {
  const outDev = devices?.outputs.find((d) => d.id === cfg.output) ?? devices?.outputs.find((d) => d.is_default);
  const inDev = devices?.inputs.find((d) => d.id === cfg.input) ?? devices?.inputs.find((d) => d.is_default);
  return (outDev?.sample_rates ?? [44100, 48000]).filter((r) => !inDev || !cfg.input_enabled || inDev.sample_rates.includes(r));
}

export interface AudioConfig {
  host: string | null;
  input: string | null;
  output: string | null;
  sample_rate: number;
  buffer_size: number;
  input_enabled: boolean;
  auto_monitor: boolean;
  recording_delay_ms: number;
  /** Threads that play tracks: 0 is Automatic, 1 the audio thread alone. */
  processing_threads: number;
  /** Lock audio memory (Linux): keep Marbble's memory in RAM so the audio never waits on swap. */
  lock_memory: boolean;
  /** Render Ahead: how far ahead, in ms, tracks that aren't played live are rendered; 0 is Off. */
  render_ahead_ms: number;
}

export interface InputLevel {
  device: string;
  volume: number;
  gain_db: number;
  muted: boolean;
  attenuating: boolean;
}
