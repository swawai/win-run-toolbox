use std::ffi::{OsString, c_void};
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_QUERY};
use windows_sys::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::launch::is_swawkit_environment_name;

pub(super) fn current_user_environment() -> io::Result<Vec<(OsString, OsString)>> {
    let token = current_process_token()?;
    let mut block = std::ptr::null_mut();
    if unsafe { CreateEnvironmentBlock(&mut block, token_handle(&token), 0) } == 0 {
        return Err(contextual_error(
            "cannot create the current-user environment block",
        ));
    }
    let block = EnvironmentBlock(block);
    // SAFETY: CreateEnvironmentBlock returned a valid double-NUL-terminated
    // UTF-16 block that remains alive through the EnvironmentBlock guard.
    Ok(unsafe { parse_environment_block(block.0.cast()) })
}

fn current_process_token() -> io::Result<OwnedHandle> {
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut token,
        )
    } == 0
    {
        return Err(contextual_error("cannot open the current-user token"));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

fn token_handle(token: &OwnedHandle) -> HANDLE {
    use std::os::windows::io::AsRawHandle;
    token.as_raw_handle() as HANDLE
}

struct EnvironmentBlock(*mut c_void);

impl Drop for EnvironmentBlock {
    fn drop(&mut self) {
        unsafe { DestroyEnvironmentBlock(self.0) };
    }
}

/// Parses one Windows environment block.
///
/// # Safety
///
/// `cursor` must point to a readable sequence of NUL-terminated UTF-16
/// entries followed by an additional NUL terminator.
unsafe fn parse_environment_block(mut cursor: *const u16) -> Vec<(OsString, OsString)> {
    let mut variables = Vec::new();
    loop {
        let mut length = 0;
        while unsafe { *cursor.add(length) } != 0 {
            length += 1;
        }
        if length == 0 {
            break;
        }
        let entry = unsafe { std::slice::from_raw_parts(cursor, length) };
        if let Some((name, value)) = split_environment_entry(entry)
            && !is_swawkit_environment_name(&name)
        {
            variables.push((name, value));
        }
        cursor = unsafe { cursor.add(length + 1) };
    }
    variables
}

fn split_environment_entry(entry: &[u16]) -> Option<(OsString, OsString)> {
    // Windows may include drive-current-directory pseudo entries such as
    // `=C:=C:\path`. `Command::env` cannot represent those names, and every
    // worker supplies an explicit current directory, so omit them.
    if entry.first() == Some(&(b'=' as u16)) {
        return None;
    }
    let separator = entry.iter().position(|value| *value == b'=' as u16)?;
    if separator == 0 {
        return None;
    }
    Some((
        OsString::from_wide(&entry[..separator]),
        OsString::from_wide(&entry[separator + 1..]),
    ))
}

fn contextual_error(action: &str) -> io::Error {
    let error = io::Error::last_os_error();
    io::Error::new(error.kind(), format!("{action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    fn block(entries: &[&str]) -> Vec<u16> {
        let mut block = Vec::new();
        for entry in entries {
            block.extend(OsStr::new(entry).encode_wide());
            block.push(0);
        }
        block.push(0);
        block
    }

    #[test]
    fn filters_only_the_owned_swawkit_namespace_case_insensitively() {
        let block = block(&[
            "SystemRoot=C:\\Windows",
            "SWAWKIT_HOME=C:\\poison",
            "swawkit_proj_module_poison=1",
            "SWAWKIT_HOME_EXTRA=foreign",
            "SWAWKIT=foreign",
            "=C:=C:\\work",
        ]);

        let parsed = unsafe { parse_environment_block(block.as_ptr()) };

        assert_eq!(
            parsed,
            [
                (OsString::from("SystemRoot"), OsString::from(r"C:\Windows")),
                (
                    OsString::from("SWAWKIT_HOME_EXTRA"),
                    OsString::from("foreign")
                ),
                (OsString::from("SWAWKIT"), OsString::from("foreign")),
            ]
        );
    }

    #[test]
    fn preserves_empty_values_and_values_containing_equals() {
        let block = block(&["EMPTY=", "FORMULA=a=b=c"]);

        assert_eq!(
            unsafe { parse_environment_block(block.as_ptr()) },
            [
                (OsString::from("EMPTY"), OsString::new()),
                (OsString::from("FORMULA"), OsString::from("a=b=c")),
            ]
        );
    }
}
