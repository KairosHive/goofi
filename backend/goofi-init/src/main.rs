//! `cargo run -p goofi-init` — the one setup step a fresh clone needs. Idempotent.
//! It draws on the screen goofi starts on, and ends on the banner and the outcome.

use goofi_screen::{banner, say, Color, Screen};

fn main() {
    let screen = Screen::begin();
    match goofi_init::init(&goofi_init::repo_root()) {
        Ok(()) => {
            screen.close(true);
            banner(Color::Green, "");
            screen.ready("Welcome to goofi. Set up");
            say("    `cargo run` starts goofi; `cargo build` and `cargo test` work too.");
        }
        Err(e) => {
            screen.close(false);
            banner(Color::Red, "");
            screen.failed("goofi-init failed", &format!("{e}\nFix the cause and run `cargo run -p goofi-init` again."));
            std::process::exit(1);
        }
    }
}
