use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

pub const ENTRY_FILE_ENV: &str = "SWAWKIT_PROJ_CORE_LAUNCH_ENTRY_FILE";
pub const LAUNCH_MODE_ENV: &str = "SWAWKIT_PROJ_CORE_LAUNCH_MODE";
pub const LAUNCH_PROTOCOL_ENV: &str = "SWAWKIT_PROJ_CORE_LAUNCH_PROTOCOL";
pub const LAUNCH_PROTOCOL_VERSION: &str = "2";
pub const WORKER_PROTOCOL_ENV: &str = "SWAWKIT_PROJ_CORE_LAUNCH_WORKER_PROTOCOL";
pub const WORKER_PROTOCOL_VERSION: &str = "1";
pub const WORKER_JOB_NAME_ENV: &str = "SWAWKIT_PROJ_CORE_LAUNCH_WORKER_JOB_NAME";
pub const WORKER_READY_EVENT_NAME_ENV: &str = "SWAWKIT_PROJ_CORE_LAUNCH_WORKER_READY_EVENT_NAME";
const PROJECT_ENVIRONMENT_PREFIX: &str = "SWAWKIT_PROJ_";
const SWAWKIT_HOME_ENV: &str = "SWAWKIT_HOME";

/// Selects the composition root without consuming a user argument.
///
/// A native launcher passes user arguments directly and selects `cli` or
/// `worker` or `internal-host` without consuming a user argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Cli,
    Worker,
    InternalHost,
}

impl LaunchMode {
    pub const fn as_env_value(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Worker => "worker",
            Self::InternalHost => "internal-host",
        }
    }
}

/// Raw, lossless facts captured at the process boundary.
///
/// This type deliberately does not resolve the entry identity or inspect the
/// filesystem. Those rules belong to `EntryContext`, after launch transport
/// details have been removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    pub mode: LaunchMode,
    pub entry_file: PathBuf,
    pub invocation_dir: PathBuf,
    pub argv: Vec<OsString>,
}

impl LaunchRequest {
    pub fn from_process() -> Result<Self, LaunchError> {
        let invocation_dir = env::current_dir().map_err(|error| {
            LaunchError::new(format!("cannot read the invocation directory: {error}"))
        })?;

        Self::from_sources(env::args_os().skip(1), invocation_dir, |name| {
            env::var_os(name)
        })
    }

    fn from_sources(
        direct_argv: impl IntoIterator<Item = OsString>,
        invocation_dir: PathBuf,
        mut lookup: impl FnMut(&str) -> Option<OsString>,
    ) -> Result<Self, LaunchError> {
        validate_launch_protocol(&mut lookup)?;
        reject_unconsumed_worker_declarations(&mut lookup)?;
        let mode = read_mode(&mut lookup)?;
        let entry_file = read_entry_file(&mut lookup)?;
        let argv = direct_argv.into_iter().collect();

        Ok(Self {
            mode,
            entry_file,
            invocation_dir,
            argv,
        })
    }
}

/// Removes inherited Swaw Kit state after the native launch declarations have
/// been captured into a [`LaunchRequest`].
///
/// # Safety
///
/// The caller must ensure that no other thread can read or mutate the process
/// environment for the duration of this call.
pub unsafe fn clear_inherited_swawkit_environment() {
    let inherited_names = env::vars_os()
        .map(|(name, _value)| name)
        .filter(|name| is_swawkit_environment_name(name))
        .collect::<Vec<_>>();

    for name in inherited_names {
        // SAFETY: the caller guarantees exclusive access to the process
        // environment for the complete snapshot-and-remove operation.
        unsafe { env::remove_var(name) };
    }
}

pub(crate) fn is_swawkit_environment_name(name: &OsStr) -> bool {
    os_ascii_eq_ignore_case(name, SWAWKIT_HOME_ENV)
        || os_has_ascii_prefix(name, PROJECT_ENVIRONMENT_PREFIX)
}

fn validate_launch_protocol(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<(), LaunchError> {
    let value = lookup(LAUNCH_PROTOCOL_ENV).ok_or_else(|| {
        LaunchError::new(format!(
            "required native Launcher protocol is missing: {LAUNCH_PROTOCOL_ENV}; rebuild or replace the Entry Launcher"
        ))
    })?;
    if value == OsStr::new(LAUNCH_PROTOCOL_VERSION) {
        return Ok(());
    }

    Err(LaunchError::new(format!(
        "unsupported {LAUNCH_PROTOCOL_ENV} value '{}'; expected '{LAUNCH_PROTOCOL_VERSION}'",
        value.to_string_lossy()
    )))
}

fn reject_unconsumed_worker_declarations(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<(), LaunchError> {
    for name in [
        WORKER_PROTOCOL_ENV,
        WORKER_JOB_NAME_ENV,
        WORKER_READY_EVENT_NAME_ENV,
    ] {
        if lookup(name).is_some() {
            return Err(LaunchError::new(format!(
                "the native Launcher did not consume its Web worker declaration: {name}; rebuild or replace the Entry Launcher"
            )));
        }
    }
    Ok(())
}

fn os_ascii_eq_ignore_case(value: &OsStr, expected: &str) -> bool {
    let mut units = value.encode_wide();
    expected.bytes().all(|expected| {
        units
            .next()
            .is_some_and(|unit| ascii_unit_eq(unit, expected))
    }) && units.next().is_none()
}

fn os_has_ascii_prefix(value: &OsStr, prefix: &str) -> bool {
    let mut units = value.encode_wide();
    prefix.bytes().all(|expected| {
        units
            .next()
            .is_some_and(|unit| ascii_unit_eq(unit, expected))
    })
}

fn ascii_unit_eq(unit: u16, expected: u8) -> bool {
    u8::try_from(unit)
        .ok()
        .is_some_and(|unit| unit.eq_ignore_ascii_case(&expected))
}

fn read_mode(lookup: &mut impl FnMut(&str) -> Option<OsString>) -> Result<LaunchMode, LaunchError> {
    let value = lookup(LAUNCH_MODE_ENV).ok_or_else(|| {
        LaunchError::new(format!(
            "required native Launcher declaration is missing: {LAUNCH_MODE_ENV}; rebuild or replace the Entry Launcher"
        ))
    })?;

    if value == OsStr::new(LaunchMode::Cli.as_env_value()) {
        return Ok(LaunchMode::Cli);
    }
    if value == OsStr::new(LaunchMode::Worker.as_env_value()) {
        return Ok(LaunchMode::Worker);
    }
    if value == OsStr::new(LaunchMode::InternalHost.as_env_value()) {
        return Ok(LaunchMode::InternalHost);
    }

    Err(LaunchError::new(format!(
        "unsupported {LAUNCH_MODE_ENV} value '{}'; expected 'cli', 'worker', or 'internal-host'",
        value.to_string_lossy()
    )))
}

fn read_entry_file(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, LaunchError> {
    let value = lookup(ENTRY_FILE_ENV).ok_or_else(|| {
        LaunchError::new(format!(
            "required launch declaration is missing: {ENTRY_FILE_ENV}"
        ))
    })?;
    if value.is_empty() {
        return Err(LaunchError::new(format!(
            "required launch declaration is missing: {ENTRY_FILE_ENV}"
        )));
    }

    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(LaunchError::new(format!(
            "launch path declaration {ENTRY_FILE_ENV} must be absolute: {}",
            path.display()
        )));
    }
    Ok(path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchError {
    message: String,
}

impl LaunchError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for LaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for LaunchError {}

#[cfg(test)]
mod tests;
