// The lists the engine answers queries with: installed plug-ins, recent projects, saved templates,
// and a project's alternatives and backups.
import type { TrackKind } from "./project";

export interface PluginDesc {
  id: string;
  name: string;
  vendor: string;
  version: string;
  subcategories: string;
  is_instrument: boolean;
}

/** File › Project Alternatives and File › Revert to (`project_files`). */
export interface ProjectFiles {
  alternatives: string[];
  active: string;
  /** The active alternative's backups, newest first. */
  backups: { file: string; time: number }[];
}

/** A template the user saved (File › Save as Template…). */
export interface UserTemplate {
  name: string;
  tracks: number;
  kinds: TrackKind[];
  tempo: number;
  /** Seconds since the epoch. */
  modified: number;
}

export interface RecentProject {
  path: string;
  name: string;
  exists: boolean;
  modified: number | null;
  tracks: number;
  tempo: number | null;
  sample_rate: number | null;
}
