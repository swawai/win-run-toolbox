use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};

static INSTALLED: AtomicBool = AtomicBool::new(false);
static REQUESTED: AtomicBool = AtomicBool::new(false);
static TERMINATION_FAILED: AtomicBool = AtomicBool::new(false);

/// Owns the process-global Windows console cancellation handler for one CLI run.
///
/// The handler itself only records the signal. The journaled process monitor then
/// terminates its directly owned command child and lets Core durably finish the
/// run journal as canceled instead of disappearing with a permanently `running`
/// record. Existing thin Launchers need no cancellation-specific behavior.
pub struct ConsoleCancellation {
    installed: bool,
}

impl ConsoleCancellation {
    pub fn install() -> io::Result<Self> {
        INSTALLED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| io::Error::other("a console cancellation handler is already active"))?;
        REQUESTED.store(false, Ordering::Release);
        TERMINATION_FAILED.store(false, Ordering::Release);
        if unsafe { SetConsoleCtrlHandler(Some(handle_console_control), 1) } == 0 {
            INSTALLED.store(false, Ordering::Release);
            return Err(io::Error::last_os_error());
        }
        Ok(Self { installed: true })
    }

    pub(crate) fn requested(&self) -> bool {
        REQUESTED.load(Ordering::Acquire)
    }

    pub(crate) fn termination_failed(&self) -> bool {
        TERMINATION_FAILED.load(Ordering::Acquire)
    }
}

pub(crate) fn requested() -> bool {
    REQUESTED.load(Ordering::Acquire)
}

pub(crate) fn mark_termination_failed() {
    TERMINATION_FAILED.store(true, Ordering::Release);
}

impl Drop for ConsoleCancellation {
    fn drop(&mut self) {
        if !self.installed {
            return;
        }
        unsafe {
            SetConsoleCtrlHandler(Some(handle_console_control), 0);
        }
        self.installed = false;
        REQUESTED.store(false, Ordering::Release);
        TERMINATION_FAILED.store(false, Ordering::Release);
        INSTALLED.store(false, Ordering::Release);
    }
}

unsafe extern "system" fn handle_console_control(control: u32) -> i32 {
    if is_cancellation(control) {
        REQUESTED.store(true, Ordering::Release);
        1
    } else {
        0
    }
}

fn is_cancellation(control: u32) -> bool {
    matches!(control, CTRL_C_EVENT | CTRL_BREAK_EVENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_c_and_ctrl_break_share_the_cli_cancellation_contract() {
        assert!(is_cancellation(CTRL_C_EVENT));
        assert!(is_cancellation(CTRL_BREAK_EVENT));
        assert!(!is_cancellation(2));
    }
}
