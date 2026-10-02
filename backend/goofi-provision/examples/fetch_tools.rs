//! Fetch every bundled tool of the manifest into `$GOOFI_RUNTIME` and say what answers:
//!   GOOFI_RUNTIME=/tmp/rt cargo run -p goofi-provision --example fetch_tools

use goofi_supervisor::layout::{self, Tool};

fn main() {
    let runtime = layout::runtime();
    println!("runtime {}", runtime.root().display());
    for tool in [Tool::Uv, Tool::Cargo, Tool::Zig, Tool::Npm, Tool::Ffmpeg] {
        match goofi_provision::ensure_tool(&runtime, tool) {
            Ok(fetched) => println!("{tool:?}: {} at {}", if fetched { "fetched" } else { "present" }, runtime.tool(tool).display()),
            Err(e) => {
                eprintln!("{tool:?}: {e}");
                std::process::exit(1);
            }
        }
        let version = match tool {
            Tool::Zig => "version",
            Tool::Ffmpeg => "-version",
            _ => "--version",
        };
        match runtime.command(tool).arg(version).output() {
            Ok(out) if out.status.success() => println!("  {}", String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or_default()),
            Ok(out) => println!("  does not answer: {}", String::from_utf8_lossy(&out.stderr).trim()),
            Err(e) => println!("  does not run: {e}"),
        }
    }
}
