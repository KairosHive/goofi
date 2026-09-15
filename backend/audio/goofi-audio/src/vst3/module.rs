//! The bundle's binary, loaded once per process and never unloaded, and its factory — asked for
//! at every use, because a factory is a reference of its own and never crosses a thread.

use std::collections::HashMap;
use std::ffi::{c_void, CStr};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use vst3::Steinberg::*;
use vst3::{ComPtr, Interface};

use super::host::cstr;

/// The factory of the binary at `path`, loading and entering it the first time. Never unloaded:
/// a bundle REPLACED in place keeps running its old code until goofi restarts (see the roadmap).
pub fn factory(path: &Path) -> Result<Factory, String> {
    static ENTERED: OnceLock<Mutex<HashMap<PathBuf, &'static libloading::Library>>> = OnceLock::new();
    let mut entered = ENTERED.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let library = match entered.get(path) {
        Some(library) => library,
        None => {
            let library = goofi_build::library(path)?;
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            enter(library, path).map_err(|e| format!("{name}: {e}"))?;
            entered.entry(path.to_path_buf()).or_insert(library)
        }
    };
    // SAFETY: `GetPluginFactory` is the VST3 module entry point, with this signature by contract.
    let get: unsafe extern "system" fn() -> *mut IPluginFactory = unsafe { goofi_build::symbol(library, c"GetPluginFactory") }?;
    unsafe { ComPtr::from_raw(get()) }.map(Factory).ok_or_else(|| "`GetPluginFactory` answered null".into())
}

/// Run the module's entry hook once, where the platform's SDK defines one; a module without it
/// is entered by loading alone.
#[cfg(unix)]
fn enter(library: &'static libloading::Library, path: &Path) -> Result<(), String> {
    let (symbol, argument) = entry_of(path);
    // SAFETY: the entry hook's signature is the VST3 SDK's, and `argument` is what it defines.
    if let Ok(entry) = unsafe { goofi_build::symbol::<unsafe extern "system" fn(*mut c_void) -> bool>(library, symbol) } {
        if !unsafe { entry(argument) } {
            return Err(format!("`{}` refused", symbol.to_string_lossy()));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn enter(library: &'static libloading::Library, _path: &Path) -> Result<(), String> {
    // SAFETY: `InitDll` is the VST3 SDK's Windows entry hook, with this signature by contract.
    if let Ok(init) = unsafe { goofi_build::symbol::<unsafe extern "system" fn() -> bool>(library, c"InitDll") } {
        if !unsafe { init() } {
            return Err("`InitDll` refused".into());
        }
    }
    Ok(())
}

/// macOS enters through the BUNDLE, and the ref is what makes it count: the SDK's `bundleEntry`
/// runs the plugin's own `InitModule` only inside `if (ref)` and answers true either way, so a null
/// one reads as a plugin that started and never did. Never released — nothing is ever unloaded.
#[cfg(target_os = "macos")]
fn entry_of(binary: &Path) -> (&'static CStr, *mut c_void) {
    use std::os::unix::ffi::OsStrExt;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            alloc: *const c_void,
            path: *const u8,
            len: isize,
            is_directory: u8,
        ) -> *const c_void;
        fn CFBundleCreate(alloc: *const c_void, url: *const c_void) -> *mut c_void;
        fn CFRelease(cf: *const c_void);
    }

    // `<bundle>/Contents/MacOS/<name>`, so the bundle is three levels up.
    let bundle = binary.ancestors().nth(3).map(|b| b.as_os_str().as_bytes()).unwrap_or_default();
    let reference = unsafe {
        let url = CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bundle.as_ptr(),
            bundle.len() as isize,
            1,
        );
        match url.is_null() {
            true => std::ptr::null_mut(),
            false => {
                let made = CFBundleCreate(std::ptr::null(), url);
                CFRelease(url);
                made
            }
        }
    };
    (c"bundleEntry", reference)
}

/// Linux's `ModuleEntry` takes the module's own handle: the one the loader opened it with.
#[cfg(all(unix, not(target_os = "macos")))]
fn entry_of(binary: &Path) -> (&'static CStr, *mut c_void) {
    (c"ModuleEntry", goofi_build::handle(binary).unwrap_or(std::ptr::null_mut()))
}

/// A class's VST3 subcategories, which name `Instrument` for a synth. A factory older than
/// `IPluginFactory2` declares none, and every one of its classes reads as an effect.
fn sub_categories(two: Option<&ComPtr<IPluginFactory2>>, index: i32) -> String {
    let Some(two) = two else { return String::new() };
    let mut info: PClassInfo2 = unsafe { std::mem::zeroed() };
    match unsafe { two.getClassInfo2(index, &mut info) } == kResultOk {
        true => cstr(&info.subCategories),
        false => String::new(),
    }
}

pub struct Factory(ComPtr<IPluginFactory>);

impl Factory {
    pub fn vendor(&self) -> String {
        let mut info: PFactoryInfo = unsafe { std::mem::zeroed() };
        unsafe { self.0.getFactoryInfo(&mut info) };
        cstr(&info.vendor)
    }

    /// Every "Audio Module Class": its cid, its name and its subcategories.
    pub fn audio_classes(&self) -> Vec<(TUID, String, String)> {
        let two: Option<ComPtr<IPluginFactory2>> = self.0.cast();
        (0..unsafe { self.0.countClasses() })
            .filter_map(|i| {
                let mut info: PClassInfo = unsafe { std::mem::zeroed() };
                let listed = unsafe { self.0.getClassInfo(i, &mut info) } == kResultOk;
                (listed && cstr(&info.category) == "Audio Module Class")
                    .then(|| (info.cid, cstr(&info.name), sub_categories(two.as_ref(), i)))
            })
            .collect()
    }

    pub fn create<I: Interface>(&self, cid: &TUID) -> Result<ComPtr<I>, String> {
        let mut obj: *mut c_void = std::ptr::null_mut();
        let result = unsafe { self.0.createInstance(cid.as_ptr(), I::IID.as_ptr() as FIDString, &mut obj) };
        unsafe { ComPtr::from_raw(obj as *mut I) }
            .filter(|_| result == kResultOk)
            .ok_or_else(|| format!("createInstance answered {result}"))
    }
}
