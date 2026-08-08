use std::ffi::OsStr;
use std::fmt::Write as _;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use sha2::{Digest, Sha256};
use swawkit_proj::entry::EntryIdentity;
use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HANDLE, SetLastError, WAIT_FAILED, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects,
};

const INSTANCE_NAME_PREFIX: &str = r"Local\SwawKit.Proj.Host.";

pub enum HostInstanceAcquisition {
    Primary(HostInstance),
    ActivatedExisting,
}

pub struct HostInstance {
    activation: Arc<OwnedHandle>,
}

impl HostInstance {
    pub fn acquire(identity: &EntryIdentity) -> io::Result<HostInstanceAcquisition> {
        // A named auto-reset event is both the per-session instance lease and
        // the activation channel. Windows creates it atomically: the creator
        // becomes primary, while later callers signal the same object.
        let name = instance_name(identity);
        let name = null_terminated(&name);
        unsafe { SetLastError(0) };
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        let creation_error = unsafe { GetLastError() };
        let activation = owned_handle(handle, "create the Entry Host activation event")?;

        if creation_error == ERROR_ALREADY_EXISTS {
            signal(
                activation.as_raw_handle() as HANDLE,
                "activate the existing Entry Host",
            )?;
            return Ok(HostInstanceAcquisition::ActivatedExisting);
        }

        Ok(HostInstanceAcquisition::Primary(Self {
            activation: Arc::new(activation),
        }))
    }

    pub fn listen<F>(&self, activate: F) -> io::Result<HostInstanceListener>
    where
        F: FnMut() -> bool + Send + 'static,
    {
        HostInstanceListener::spawn(Arc::clone(&self.activation), activate)
    }
}

pub struct HostInstanceListener {
    stop: Arc<OwnedHandle>,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl HostInstanceListener {
    fn spawn<F>(activation: Arc<OwnedHandle>, mut activate: F) -> io::Result<Self>
    where
        F: FnMut() -> bool + Send + 'static,
    {
        let stop = Arc::new(owned_handle(
            unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) },
            "create the Entry Host activation listener stop event",
        )?);
        let listener_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("swawkit-host-activation".to_owned())
            .spawn(move || {
                let handles = [
                    listener_stop.as_raw_handle() as HANDLE,
                    activation.as_raw_handle() as HANDLE,
                ];
                loop {
                    let result = unsafe {
                        WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, INFINITE)
                    };
                    match result {
                        WAIT_OBJECT_0 => return Ok(()),
                        value if value == WAIT_OBJECT_0 + 1 => {
                            if !activate() {
                                return Ok(());
                            }
                        }
                        WAIT_FAILED => return Err(io::Error::last_os_error()),
                        value => {
                            return Err(io::Error::other(format!(
                                "unexpected Entry Host activation wait result: {value}"
                            )));
                        }
                    }
                }
            })?;

        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    pub fn stop(mut self) -> io::Result<()> {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> io::Result<()> {
        if self.thread.is_none() {
            return Ok(());
        }
        signal(
            self.stop.as_raw_handle() as HANDLE,
            "stop the Entry Host activation listener",
        )?;
        let thread = self
            .thread
            .take()
            .expect("the activation listener was checked above");
        thread
            .join()
            .map_err(|_| io::Error::other("Entry Host activation listener panicked"))?
    }
}

impl Drop for HostInstanceListener {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let _ = self.stop_and_join();
        }
    }
}

fn instance_name(identity: &EntryIdentity) -> String {
    let digest = Sha256::digest(identity.key().as_bytes());
    let mut name = String::with_capacity(INSTANCE_NAME_PREFIX.len() + digest.len() * 2);
    name.push_str(INSTANCE_NAME_PREFIX);
    for byte in digest {
        write!(&mut name, "{byte:02x}").expect("writing to a String cannot fail");
    }
    name
}

fn null_terminated(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn owned_handle(handle: HANDLE, action: &str) -> io::Result<OwnedHandle> {
    if handle.is_null() {
        let error = io::Error::last_os_error();
        return Err(io::Error::new(
            error.kind(),
            format!("cannot {action}: {error}"),
        ));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn signal(handle: HANDLE, action: &str) -> io::Result<()> {
    if unsafe { SetEvent(handle) } == 0 {
        let error = io::Error::last_os_error();
        return Err(io::Error::new(
            error.kind(),
            format!("cannot {action}: {error}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    const TEST_FILE_ID_ENV: &str = "SWAWKIT_PROJ_TEST_HOST_INSTANCE_FILE_ID";
    const TEST_VOLUME_ID: &str = r"\\?\volume{91cf565a-694f-4232-be2d-368578d28629}";
    static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn a_second_claim_activates_the_primary_instance() {
        let identity = unique_identity();
        let HostInstanceAcquisition::Primary(primary) = HostInstance::acquire(&identity).unwrap()
        else {
            panic!("the first claim must become primary");
        };
        let (activated, received) = mpsc::channel();
        let listener = primary.listen(move || activated.send(()).is_ok()).unwrap();

        assert!(matches!(
            HostInstance::acquire(&identity).unwrap(),
            HostInstanceAcquisition::ActivatedExisting
        ));
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("primary receives activation");

        listener.stop().unwrap();
    }

    #[test]
    fn activation_is_retained_until_the_primary_starts_listening() {
        let identity = unique_identity();
        let HostInstanceAcquisition::Primary(primary) = HostInstance::acquire(&identity).unwrap()
        else {
            panic!("the first claim must become primary");
        };
        assert!(matches!(
            HostInstance::acquire(&identity).unwrap(),
            HostInstanceAcquisition::ActivatedExisting
        ));

        let (activated, received) = mpsc::channel();
        let listener = primary.listen(move || activated.send(()).is_ok()).unwrap();
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("the pending activation is retained");

        listener.stop().unwrap();
    }

    #[test]
    fn different_entry_identities_have_independent_hosts() {
        let first = unique_identity();
        let second = unique_identity();

        let HostInstanceAcquisition::Primary(first_primary) =
            HostInstance::acquire(&first).unwrap()
        else {
            panic!("the first Entry must have its own primary");
        };
        let HostInstanceAcquisition::Primary(second_primary) =
            HostInstance::acquire(&second).unwrap()
        else {
            panic!("the second Entry must have its own primary");
        };

        assert!(matches!(
            HostInstance::acquire(&first).unwrap(),
            HostInstanceAcquisition::ActivatedExisting
        ));
        assert!(matches!(
            HostInstance::acquire(&second).unwrap(),
            HostInstanceAcquisition::ActivatedExisting
        ));
        drop((first_primary, second_primary));
    }

    #[test]
    fn releasing_the_primary_allows_a_new_primary() {
        let identity = unique_identity();
        let HostInstanceAcquisition::Primary(primary) = HostInstance::acquire(&identity).unwrap()
        else {
            panic!("the first claim must become primary");
        };
        drop(primary);

        assert!(matches!(
            HostInstance::acquire(&identity).unwrap(),
            HostInstanceAcquisition::Primary(_)
        ));
    }

    #[test]
    fn another_process_activates_the_primary_instance() {
        let identity = unique_identity();
        let HostInstanceAcquisition::Primary(primary) = HostInstance::acquire(&identity).unwrap()
        else {
            panic!("the parent process must become primary");
        };
        let (activated, received) = mpsc::channel();
        let listener = primary.listen(move || activated.send(()).is_ok()).unwrap();

        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "host_instance::tests::subprocess_activation_helper",
                "--nocapture",
            ])
            .env(TEST_FILE_ID_ENV, identity.file_id())
            .output()
            .expect("start the secondary test process");
        assert!(
            output.status.success(),
            "secondary process failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("primary receives cross-process activation");

        listener.stop().unwrap();
    }

    #[test]
    fn subprocess_activation_helper() {
        let Some(file_id) = std::env::var_os(TEST_FILE_ID_ENV) else {
            return;
        };
        let identity =
            EntryIdentity::from_parts(TEST_VOLUME_ID, file_id.to_string_lossy()).unwrap();

        assert!(matches!(
            HostInstance::acquire(&identity).unwrap(),
            HostInstanceAcquisition::ActivatedExisting
        ));
    }

    fn unique_identity() -> EntryIdentity {
        let sequence = NEXT_IDENTITY.fetch_add(1, Ordering::Relaxed);
        let file_id = format!(
            "{:032x}",
            ((std::process::id() as u128) << 64) | sequence as u128
        );
        EntryIdentity::from_parts(TEST_VOLUME_ID, file_id).unwrap()
    }
}
