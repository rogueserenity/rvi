//! Unix signal handling for terminal events.
//!
//! Provides a safe, atomic flag for SIGWINCH (terminal resize) detection.
//! Install once at startup with `install_sigwinch_handler`; poll cheaply
//! with `sigwinch_received` on each event loop iteration.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::error::TerminalError;

static SIGWINCH_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Install a SIGWINCH handler that sets an atomic flag on resize.
///
/// Call once at editor startup. Safe to call multiple times; the flag is
/// initialized only once and the handler is re-registered on each call.
///
/// # Errors
///
/// Returns an error if the signal handler cannot be registered.
pub fn install_sigwinch_handler() -> Result<(), TerminalError> {
    let flag = SIGWINCH_FLAG.get_or_init(|| Arc::new(AtomicBool::new(false)));
    signal_hook::flag::register(signal_hook::consts::SIGWINCH, Arc::clone(flag))
        .map_err(TerminalError::ReadInput)?;
    Ok(())
}

/// Returns `true` and clears the flag if a SIGWINCH has been received since
/// the last call. Returns `false` if the handler was never installed.
pub fn sigwinch_received() -> bool {
    SIGWINCH_FLAG
        .get()
        .map(|flag| flag.swap(false, Ordering::AcqRel))
        .unwrap_or(false)
}
