//! Every situation that can share a process, one binary: libtest runs them in parallel and the
//! workspace links once. A situation that sets process-wide environment keeps its own binary.

/// The libtest name of a module's test: its path under the crate root, as a filter spells it.
pub fn situation(module_path: &str) -> &str {
    module_path.split_once("::").map_or(module_path, |(_, rest)| rest)
}

#[cfg(target_os = "linux")]
mod audio_priority;
mod browser;
#[cfg(not(feature = "embed"))]
mod bundles;
#[cfg(feature = "embed")]
mod bundles_expr;
mod children;
mod codec_golden;
mod contracts;
mod demo;
mod editing;
#[cfg(not(feature = "embed"))]
mod eeg;
mod engines;
mod filter_golden;
mod fluxrt;
mod graphics;
mod harmonic_geometry;
mod headless;
mod image_file;
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
mod python_gil_tripwire;
#[cfg(feature = "embed")]
mod python_init_order;
#[cfg(feature = "embed")]
mod python_module_hygiene;
mod recording;
mod running;
mod session;
mod signals;
mod simulation;
mod subpatches;
mod textures;
mod transport;
mod upscaling;
mod upscaling_fluxrt;
