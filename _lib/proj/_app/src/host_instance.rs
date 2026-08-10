use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};

use swawkit_proj::entry::EntryIdentity;
use swawkit_proj::host_runtime::entry_key_sha256;
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, SetLastError};
use windows_sys::Win32::System::Threading::CreateEventW;

const INSTANCE_NAME_PREFIX: &str = r"Local\SwawKit.Proj.Host.";

pub enum HostInstanceAcquisition {
    Primary(HostInstance),
    Existing,
}

pub struct HostInstance {
    _lease: OwnedHandle,
}

impl HostInstance {
    pub fn acquire(identity: &EntryIdentity) -> io::Result<HostInstanceAcquisition> {
        // The named kernel object is only an atomic per-session lease. Runtime
        // discovery and activation are separate, acknowledged protocols.
        let name = null_terminated(&format!(
            "{INSTANCE_NAME_PREFIX}{}",
            entry_key_sha256(identity)
        ));
        unsafe { SetLastError(0) };
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        let creation_error = unsafe { GetLastError() };
        let lease = owned_handle(handle, "create the Entry Host instance lease")?;

        if creation_error == ERROR_ALREADY_EXISTS {
            return Ok(HostInstanceAcquisition::Existing);
        }

        Ok(HostInstanceAcquisition::Primary(Self { _lease: lease }))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const TEST_FILE_ID_ENV: &str = "SWAWKIT_PROJ_TEST_HOST_INSTANCE_FILE_ID";
    const TEST_VOLUME_ID: &str = r"\\?\volume{91cf565a-694f-4232-be2d-368578d28629}";
    static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn a_second_claim_observes_the_existing_instance() {
        let identity = unique_identity();
        let HostInstanceAcquisition::Primary(_primary) = HostInstance::acquire(&identity).unwrap()
        else {
            panic!("the first claim must become primary");
        };

        assert!(matches!(
            HostInstance::acquire(&identity).unwrap(),
            HostInstanceAcquisition::Existing
        ));
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
            HostInstanceAcquisition::Existing
        ));
        assert!(matches!(
            HostInstance::acquire(&second).unwrap(),
            HostInstanceAcquisition::Existing
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
    fn another_process_observes_the_existing_instance() {
        let identity = unique_identity();
        let HostInstanceAcquisition::Primary(_primary) = HostInstance::acquire(&identity).unwrap()
        else {
            panic!("the parent process must become primary");
        };

        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "host_instance::tests::subprocess_existing_helper",
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
    }

    #[test]
    fn subprocess_existing_helper() {
        let Some(file_id) = std::env::var_os(TEST_FILE_ID_ENV) else {
            return;
        };
        let identity =
            EntryIdentity::from_parts(TEST_VOLUME_ID, file_id.to_string_lossy()).unwrap();

        assert!(matches!(
            HostInstance::acquire(&identity).unwrap(),
            HostInstanceAcquisition::Existing
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
