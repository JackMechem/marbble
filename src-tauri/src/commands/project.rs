//! Project files: new, save, open, recent projects and the Open dialog's folder listing, crash
//! recovery, project alternatives, backups and templates.

use super::*;

#[tauri::command]
pub fn new_project(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.new_project())
}

#[tauri::command]
pub fn save_project(state: State<AppState>, path: Option<String>) -> R<Snapshot> {
    try_snap(&state, |s| {
        let saved = s.save(path.map(PathBuf::from))?;
        crate::recent::add(&saved);
        Ok(())
    })
}

#[tauri::command]
pub fn open_project(state: State<AppState>, path: String) -> R<Snapshot> {
    try_snap(&state, |s| {
        s.open(&PathBuf::from(&path))?;
        crate::recent::add(&s.project_path());
        Ok(())
    })
}

#[tauri::command(async)]
pub fn recent_projects() -> Vec<crate::recent::RecentProject> {
    crate::recent::list()
}

#[tauri::command]
pub fn forget_recent(path: String) {
    crate::recent::remove(&path)
}

#[tauri::command]
pub fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

/// Where new projects go unless the user picks somewhere else (Settings › General).
#[tauri::command]
pub fn default_project_dir(state: State<AppState>) -> String {
    with(&state, |s| s.projects_root()).to_string_lossy().to_string()
}

/// The projects folder from before the rename, while it can be renamed (Settings › General).
#[derive(Serialize)]
pub struct OldProjectsFolder {
    old: String,
    new: String,
    /// The open project, when it is in the old folder and so holds it.
    open: Option<String>,
}

#[tauri::command]
pub fn old_projects_folder(state: State<AppState>) -> Option<OldProjectsFolder> {
    let (old, new) = daw_engine::old_name::projects_rename()?;
    let dir = with(&state, |s| s.project_path());
    let open = (dir.exists() && dir.starts_with(&old)).then(|| dir.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    Some(OldProjectsFolder { old: old.to_string_lossy().to_string(), new: new.to_string_lossy().to_string(), open })
}

/// Renames `~/Music/Fermata` to `~/Music/Marbble`, then points the recent projects and the
/// folder settings at the new place.
#[tauri::command]
pub fn rename_old_projects_folder(state: State<AppState>) -> R<Snapshot> {
    with(&state, |s| {
        let pairs = s.rename_old_projects_folder().map_err(|e| e.to_string())?;
        crate::recent::rebase(&pairs);
        let mut all = crate::settings::load();
        if daw_engine::old_name::rebase_prefs(&mut all.prefs, &pairs) {
            crate::settings::save(&all);
            s.set_prefs(all.prefs);
        }
        Ok(snapshot(s))
    })
}

#[derive(Serialize)]
pub struct DirEntry {
    name: String,
    path: String,
    is_dir: bool,
}

#[derive(Serialize)]
pub struct DirListing {
    path: String,
    parent: Option<String>,
    entries: Vec<DirEntry>,
}

/// Lists folders and audio files for the All Files browser.
#[tauri::command(async)]
pub fn list_dir(path: Option<String>) -> R<DirListing> {
    let dir = match path {
        Some(p) => PathBuf::from(p),
        None => {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
            let music = home.join("Music");
            if music.is_dir() {
                music
            } else {
                home
            }
        }
    };
    let mut entries: Vec<DirEntry> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None;
            }
            let p = e.path();
            let is_dir = p.is_dir();
            let audio = daw_engine::decode::is_audio_file(&p);
            (is_dir || audio).then(|| DirEntry { name, path: p.to_string_lossy().to_string(), is_dir })
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(DirListing { parent: dir.parent().map(|p| p.to_string_lossy().to_string()), path: dir.to_string_lossy().to_string(), entries })
}

#[tauri::command]
pub fn set_project_notes(state: State<AppState>, notes: String) -> Snapshot {
    edit(&state, "Project Notes", Some("notes".into()), false, |s| s.set_notes(notes))
}

/// Whether the project at `path` has an autosave newer than its last save.
#[tauri::command]
pub fn recovery_info(path: String) -> Option<daw_engine::session::Recovery> {
    daw_engine::session::recovery_info(std::path::Path::new(&path))
}

/// Projects with unsaved work left behind (after a crash): recent projects, plus project
/// folders in the projects folder (where untitled projects live).
#[tauri::command(async)]
pub fn recoverable_projects(state: State<AppState>) -> Vec<daw_engine::session::Recovery> {
    let (root, current) = with(&state, |s| (s.projects_root(), s.project_path()));
    let mut dirs: Vec<PathBuf> = crate::recent::paths().into_iter().map(PathBuf::from).collect();
    for r in [root, daw_engine::session::default_projects_root()] {
        if let Ok(rd) = std::fs::read_dir(&r) {
            dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| daw_engine::old_name::is_project(p)));
        }
    }
    let mut seen = Vec::new();
    let mut out: Vec<_> = dirs
        .into_iter()
        .filter(|d| *d != current)
        .filter(|d| {
            let k = std::fs::canonicalize(d).unwrap_or_else(|_| d.clone());
            !seen.contains(&k) && {
                seen.push(k);
                true
            }
        })
        .filter_map(|d| daw_engine::session::recovery_info(&d))
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.autosaved));
    out
}

/// Opens a project from its autosave.
#[tauri::command]
pub fn open_recovered(state: State<AppState>, path: String) -> R<Snapshot> {
    try_snap(&state, |s| {
        s.open_recovered(&PathBuf::from(&path))?;
        if s.saved {
            crate::recent::add(&s.project_path());
        }
        Ok(())
    })
}

/// Deletes an autosave: the given project's, or the current one's.
#[tauri::command]
pub fn discard_autosave(state: State<AppState>, path: Option<String>) {
    match path {
        Some(p) => daw_engine::session::discard_autosave(std::path::Path::new(&p)),
        None => with(&state, |s| s.discard_autosave()),
    }
}

/// What File › Project Alternatives and File › Revert to list.
#[derive(Serialize)]
pub struct ProjectFiles {
    alternatives: Vec<String>,
    active: String,
    /// The active alternative's backups, newest first.
    backups: Vec<daw_engine::session::BackupInfo>,
}

#[tauri::command]
pub fn project_files(state: State<AppState>) -> ProjectFiles {
    with(&state, |s| ProjectFiles { alternatives: s.alternatives(), active: s.alternative_name(), backups: s.backups() })
}

/// File › Project Alternatives › New Alternative: saves, then goes on as a copy named `name`.
#[tauri::command]
pub fn new_alternative(state: State<AppState>, name: String) -> R<Snapshot> {
    try_snap(&state, |s| s.new_alternative(&name))
}

/// Switches to another project alternative, saving the active one first.
#[tauri::command]
pub fn switch_alternative(state: State<AppState>, name: String) -> R<Snapshot> {
    try_snap(&state, |s| s.switch_alternative(&name))
}

#[tauri::command]
pub fn rename_alternative(state: State<AppState>, name: String, new_name: String) -> R<Snapshot> {
    try_snap(&state, |s| s.rename_alternative(&name, &new_name))
}

#[tauri::command]
pub fn delete_alternative(state: State<AppState>, name: String) -> R<Snapshot> {
    try_snap(&state, |s| s.delete_alternative(&name))
}

/// File › Revert to Saved.
#[tauri::command]
pub fn revert_to_saved(state: State<AppState>) -> R<Snapshot> {
    try_snap(&state, |s| s.revert_to_saved())
}

/// File › Revert to › a backup (`file` as `project_files` lists it).
#[tauri::command]
pub fn revert_to_backup(state: State<AppState>, file: String) -> R<Snapshot> {
    try_snap(&state, |s| s.revert_to_backup(&file))
}

/// File › Save as Template: into the config folder's templates.
#[tauri::command]
pub fn save_template(state: State<AppState>, name: String, replace: bool) -> R<()> {
    with(&state, |s| s.save_template(&daw_engine::session::templates_dir(), &name, replace)).map(|_| ()).map_err(|e| e.to_string())
}

/// The user's templates, for the Project Chooser's My Templates.
#[tauri::command(async)]
pub fn user_templates() -> Vec<daw_engine::session::UserTemplate> {
    daw_engine::session::list_templates(&daw_engine::session::templates_dir())
}

#[tauri::command]
pub fn delete_template(name: String) -> R<()> {
    daw_engine::session::delete_template(&daw_engine::session::templates_dir(), &name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_template(name: String, new_name: String) -> R<()> {
    daw_engine::session::rename_template(&daw_engine::session::templates_dir(), &name, &new_name).map_err(|e| e.to_string())
}

/// Starts a new, untitled project from one of the user's templates.
#[tauri::command]
pub fn new_from_template(state: State<AppState>, name: String) -> R<Snapshot> {
    try_snap(&state, |s| s.new_from_template(&daw_engine::session::templates_dir(), &name))
}
