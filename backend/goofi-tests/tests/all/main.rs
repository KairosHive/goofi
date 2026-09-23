//! Every situation that can share a process, one binary: libtest runs them in parallel and the
//! workspace links once. A situation that sets process-wide environment keeps its own binary.

/// The libtest name of a module's test: its path under the crate root, as a filter spells it.
pub fn situation(module_path: &str) -> &str {
    module_path.split_once("::").map_or(module_path, |(_, rest)| rest)
}

#[cfg(target_os = "linux")]
mod audio_priority;
mod browser;
mod children;
mod codec_golden;
mod contracts;
mod demo;
mod editing;
mod engines;
mod filter_golden;
mod graphics;
mod headless;
mod inspect;
#[cfg(not(feature = "embed"))]
mod io;
mod logging;
mod nodes;
#[cfg(feature = "embed")]
mod param_modulation;
mod plugins;
mod python;
#[cfg(feature = "embed")]
mod python_init_order;
#[cfg(feature = "embed")]
mod python_module_hygiene;
mod recording;
mod running;
mod session;
mod signals;
mod subpatches;
mod textures;
mod transport;
