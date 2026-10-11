//! What a goofi process asks of its platform once, for its whole life. Each call is a no-op where
//! the platform already behaves so.

/// Run `stop` when the console window closes, the user logs off or the machine goes down. The
/// handler returns only once the process has ended: Windows grants the grace it gives a console
/// process (5 s on close, 20 s on shutdown) for exactly as long as the handler runs, so one that
/// returned at once would hand the orderly shutdown to a kill. Unix has signals for this.
#[cfg(windows)]
pub fn on_console_close(stop: impl Fn() + Send + Sync + 'static) -> Result<(), String> {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT};

    static STOP: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
    unsafe extern "system" fn handler(event: u32) -> i32 {
        if !matches!(event, CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT) {
            // Ctrl+C and Ctrl+Break are the next handler's.
            return 0;
        }
        if let Some(stop) = STOP.get() {
            stop();
        }
        // The shutdown ends the process from its own thread; this one holds the grace open.
        loop {
            std::thread::park();
        }
    }
    STOP.set(Box::new(stop)).map_err(|_| "the console close handler is already set".to_string())?;
    // SAFETY: registering a plain function; the routine touches only the static above.
    if unsafe { SetConsoleCtrlHandler(Some(handler), 1) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

/// Ask for 1 ms timer resolution for the life of the process. Windows rounds every timed wait up
/// to its 15.6 ms tick otherwise, which the executor's 10 ms tick and a clock above 60 Hz cannot
/// live with. The request is released when the process ends.
pub fn fine_timer() {
    #[cfg(windows)]
    // SAFETY: a plain call with a period in milliseconds.
    unsafe {
        windows_sys::Win32::Media::timeBeginPeriod(1);
    }
}

/// Fail a crash without a dialog: a child meant to be WATCHED — a plugin scan — must exit so its
/// owner can report it, not wait for a click nobody gives.
pub fn no_error_dialogs() {
    #[cfg(windows)]
    // SAFETY: a plain call that sets this process's error mode.
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX};
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    }
}

/// Whether an exit code is an NTSTATUS failure — an access violation, a stack overflow — which
/// Windows reports as the exit code of a process that crashed. Never so elsewhere.
pub fn is_crash_code(code: i32) -> bool {
    cfg!(windows) && (code as u32) & 0xC000_0000 == 0xC000_0000
}
