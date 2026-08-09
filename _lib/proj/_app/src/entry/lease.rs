use std::fs::File;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::identity::{EntryIdentity, EntryIdentityError, IdentityPathKind, open_identity};

pub(crate) struct EntryIdentityLease {
    _file: File,
    identity: EntryIdentity,
}

impl EntryIdentityLease {
    pub(crate) fn acquire_entry(path: &Path) -> Result<Self, EntryIdentityError> {
        Self::acquire(
            path,
            IdentityPathKind::File,
            FILE_GENERIC_READ,
            FILE_SHARE_READ,
        )
    }

    pub(crate) fn acquire_directory(path: &Path) -> Result<Self, EntryIdentityError> {
        Self::acquire(
            path,
            IdentityPathKind::Directory,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
    }

    fn acquire(
        path: &Path,
        kind: IdentityPathKind,
        desired_access: u32,
        share_mode: u32,
    ) -> Result<Self, EntryIdentityError> {
        let (file, identity) = open_identity(path, kind, desired_access, share_mode)?;
        Ok(Self {
            _file: file,
            identity,
        })
    }

    pub(crate) fn identity(&self) -> &EntryIdentity {
        &self.identity
    }
}
