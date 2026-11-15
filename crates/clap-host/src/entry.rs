//! Loading a `.clap` file and reading its plug-in factory.

use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use clap_sys::entry::clap_plugin_entry;
use clap_sys::factory::plugin_factory::{clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID};
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::version::clap_version_is_compatible;
use parking_lot::Mutex;
use plugin_api::{PluginDescriptor, PluginFormat};

use crate::features;

/// A loaded `.clap` file whose entry has been initialized. Cached for the life of the process and
/// never unloaded (its `deinit` is never called), as VST3 modules are: plug-ins don't all survive
/// being closed and opened again.
pub(crate) struct Library {
    pub path: PathBuf,
    factory: *const clap_plugin_factory,
    _lib: libloading::Library,
}

unsafe impl Send for Library {}
unsafe impl Sync for Library {}

static LIBRARIES: OnceLock<Mutex<HashMap<PathBuf, Arc<Library>>>> = OnceLock::new();

/// Loads a `.clap` file, or gives the one loaded before.
pub(crate) fn load(path: &Path) -> Result<Arc<Library>, String> {
    let cache = LIBRARIES.get_or_init(Default::default);
    if let Some(l) = cache.lock().get(path) {
        return Ok(l.clone());
    }
    let lib = Arc::new(unsafe { Library::open(path)? });
    cache.lock().insert(path.to_path_buf(), lib.clone());
    Ok(lib)
}

/// The binary to open: the file itself, or on macOS the executable inside the bundle.
fn binary_path(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let dir = path.join("Contents/MacOS");
    let stem = path.file_stem()?;
    let direct = dir.join(stem);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).find(|p| p.is_file())
}

/// A loader error with its cause: libloading's own message is just "dlopen failed".
fn load_error(e: libloading::Error) -> String {
    match std::error::Error::source(&e) {
        Some(cause) => format!("{e}: {cause}"),
        None => e.to_string(),
    }
}

impl Library {
    unsafe fn open(path: &Path) -> Result<Library, String> {
        let bin = binary_path(path).ok_or_else(|| format!("no binary found in {}", path.display()))?;
        let lib = libloading::Library::new(&bin).map_err(load_error)?;
        let entry = *lib.get::<*const clap_plugin_entry>(b"clap_entry\0").map_err(|e| format!("not a CLAP plug-in ({})", load_error(e)))?;
        let entry = entry.as_ref().ok_or("its clap_entry is null")?;
        if !clap_version_is_compatible(entry.clap_version) {
            let v = entry.clap_version;
            return Err(format!("it was made for CLAP {}.{}.{}, which this host doesn't speak", v.major, v.minor, v.revision));
        }
        // The entry is given the path of the file or bundle, not of the binary inside it.
        let c_path = CString::new(path.to_string_lossy().as_bytes()).map_err(|_| "a NUL in its path")?;
        if !entry.init.is_some_and(|init| init(c_path.as_ptr())) {
            return Err("its entry refused to initialize".into());
        }
        let factory = entry.get_factory.map(|f| f(CLAP_PLUGIN_FACTORY_ID.as_ptr())).unwrap_or(std::ptr::null());
        if factory.is_null() {
            return Err("it has no plug-in factory".into());
        }
        Ok(Library { path: path.to_path_buf(), factory: factory as *const clap_plugin_factory, _lib: lib })
    }

    /// The plug-ins its factory lists.
    pub fn descriptors(&self) -> Vec<PluginDescriptor> {
        let mut out = Vec::new();
        unsafe {
            let f = &*self.factory;
            let count = f.get_plugin_count.map_or(0, |c| c(self.factory));
            for i in 0..count {
                let Some(d) = f.get_plugin_descriptor.and_then(|g| g(self.factory, i).as_ref()) else { continue };
                if !clap_version_is_compatible(d.clap_version) || d.id.is_null() {
                    continue;
                }
                let feats = string_list(d.features);
                let id = text(d.id);
                out.push(PluginDescriptor {
                    id: format!("{}|{id}", self.path.display()),
                    name: text(d.name),
                    vendor: text(d.vendor),
                    version: text(d.version),
                    path: self.path.clone(),
                    cid: id,
                    subcategories: features::subcategories(&feats),
                    is_instrument: features::is_instrument(&feats),
                    format: PluginFormat::Clap,
                });
            }
        }
        out
    }

    /// Creates the plug-in with this id, not yet initialized; null if the factory can't.
    pub unsafe fn create(&self, host: *const clap_host, id: &CStr) -> *const clap_plugin {
        match (*self.factory).create_plugin {
            Some(create) => create(self.factory, host, id.as_ptr()),
            None => std::ptr::null(),
        }
    }
}

/// A C string the plug-in gave, or "" for null.
pub(crate) unsafe fn text(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// A null-terminated list of C strings.
unsafe fn string_list(mut p: *const *const c_char) -> Vec<String> {
    let mut out = Vec::new();
    while !p.is_null() && !(*p).is_null() {
        out.push(text(*p));
        p = p.add(1);
    }
    out
}

/// A fixed-size C string buffer from a plug-in's info struct.
pub(crate) fn buf_text(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
