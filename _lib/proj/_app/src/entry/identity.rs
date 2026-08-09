use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs::File;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::Path;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FileAttributeTagInfo, FileIdInfo, GetFileInformationByHandleEx,
    GetVolumeNameForVolumeMountPointW, OPEN_EXISTING,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryIdentity {
    volume_id: String,
    file_id: String,
}

impl EntryIdentity {
    pub fn read(entry_file: &Path) -> Result<Self, EntryIdentityError> {
        Self::read_path(entry_file, IdentityPathKind::File)
    }

    pub(crate) fn read_directory(directory: &Path) -> Result<Self, EntryIdentityError> {
        Self::read_path(directory, IdentityPathKind::Directory)
    }

    fn read_path(path: &Path, kind: IdentityPathKind) -> Result<Self, EntryIdentityError> {
        let (_, identity) = open_identity(
            path,
            kind,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )?;
        Ok(identity)
    }

    pub fn from_parts(
        volume_id: impl Into<String>,
        file_id: impl Into<String>,
    ) -> Result<Self, EntryIdentityError> {
        let identity = Self {
            volume_id: volume_id.into(),
            file_id: file_id.into(),
        };
        if !is_valid_volume_id(&identity.volume_id) {
            return Err(EntryIdentityError::new("volumeId is invalid".to_owned()));
        }
        if !is_valid_file_id(&identity.file_id) {
            return Err(EntryIdentityError::new("fileId is invalid".to_owned()));
        }
        Ok(identity)
    }

    pub fn volume_id(&self) -> &str {
        &self.volume_id
    }

    pub fn file_id(&self) -> &str {
        &self.file_id
    }

    pub fn key(&self) -> String {
        format!("{}|{}", self.volume_id, self.file_id)
    }
}

pub(super) fn open_identity(
    path: &Path,
    kind: IdentityPathKind,
    desired_access: u32,
    share_mode: u32,
) -> Result<(File, EntryIdentity), EntryIdentityError> {
    let path = std::path::absolute(path).map_err(|error| {
        EntryIdentityError::new(format!(
            "invalid {} path '{}': {error}",
            kind.label(),
            path.display()
        ))
    })?;
    if !kind.matches(&path) {
        return Err(EntryIdentityError::new(format!(
            "{} does not exist: {}",
            kind.label(),
            path.display()
        )));
    }

    let file = open_identity_handle(&path, kind, desired_access, share_mode)?;
    validate_identity_handle(&file, &path, kind)?;
    let file_id = read_file_id(&file, &path)?;
    let volume_id = read_volume_id(&path)?;
    Ok((file, EntryIdentity { volume_id, file_id }))
}

#[derive(Clone, Copy)]
pub(super) enum IdentityPathKind {
    File,
    Directory,
}

impl IdentityPathKind {
    fn label(self) -> &'static str {
        match self {
            Self::File => "project entry file",
            Self::Directory => "project DataRoot directory",
        }
    }

    fn matches(self, path: &Path) -> bool {
        match self {
            Self::File => path.is_file(),
            Self::Directory => path.is_dir(),
        }
    }

    fn open_flags(self) -> u32 {
        FILE_FLAG_OPEN_REPARSE_POINT
            | match self {
                Self::File => 0,
                Self::Directory => FILE_FLAG_BACKUP_SEMANTICS,
            }
    }
}

pub(crate) fn is_valid_volume_id(value: &str) -> bool {
    let lowercase = value.to_ascii_lowercase();
    let Some(body) = lowercase
        .strip_prefix(r"\\?\volume{")
        .and_then(|value| value.strip_suffix('}'))
    else {
        return false;
    };
    !body.is_empty() && body.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

pub(crate) fn is_valid_file_id(value: &str) -> bool {
    (16..=32).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn open_identity_handle(
    path: &Path,
    kind: IdentityPathKind,
    desired_access: u32,
    share_mode: u32,
) -> Result<File, EntryIdentityError> {
    let path_wide = null_terminated(path.as_os_str())?;
    let handle = unsafe {
        CreateFileW(
            path_wide.as_ptr(),
            desired_access,
            share_mode,
            std::ptr::null(),
            OPEN_EXISTING,
            kind.open_flags(),
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(last_os_error(
            &format!("cannot open {} for identity", kind.label()),
            path,
        ));
    }
    Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
}

fn validate_identity_handle(
    file: &File,
    path: &Path,
    kind: IdentityPathKind,
) -> Result<(), EntryIdentityError> {
    let mut attributes = FILE_ATTRIBUTE_TAG_INFO::default();
    query_file_information(file, FileAttributeTagInfo, &mut attributes, path)?;
    if attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(EntryIdentityError::new(format!(
            "{} cannot be a reparse point: {}",
            kind.label(),
            path.display()
        )));
    }
    let is_directory = attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    if is_directory != matches!(kind, IdentityPathKind::Directory) {
        return Err(EntryIdentityError::new(format!(
            "{} changed type while it was opened: {}",
            kind.label(),
            path.display()
        )));
    }
    Ok(())
}

fn read_file_id(file: &File, path: &Path) -> Result<String, EntryIdentityError> {
    let mut information = FILE_ID_INFO::default();
    query_file_information(file, FileIdInfo, &mut information, path)?;
    // The v0 record follows fsutil's numeric rendering, which reverses the
    // little-endian FILE_ID_128 byte buffer returned by Windows.
    Ok(information
        .FileId
        .Identifier
        .iter()
        .rev()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn query_file_information<T>(
    file: &File,
    class: i32,
    output: &mut T,
    path: &Path,
) -> Result<(), EntryIdentityError> {
    use std::os::windows::io::AsRawHandle;

    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as _,
            class,
            (output as *mut T).cast(),
            size_of::<T>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(last_os_error("cannot query filesystem identity", path));
    }
    Ok(())
}

fn read_volume_id(path: &Path) -> Result<String, EntryIdentityError> {
    let volume_root = path.ancestors().last().ok_or_else(|| {
        EntryIdentityError::new(format!("path has no volume root: {}", path.display()))
    })?;
    let root_wide = null_terminated(volume_root.as_os_str())?;
    let mut volume_name = [0_u16; 64];
    let succeeded = unsafe {
        GetVolumeNameForVolumeMountPointW(
            root_wide.as_ptr(),
            volume_name.as_mut_ptr(),
            volume_name.len() as u32,
        )
    };
    if succeeded == 0 {
        return Err(last_os_error("cannot query filesystem volume identity", path));
    }
    let length = volume_name
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(volume_name.len());
    let value = String::from_utf16(&volume_name[..length])
        .map_err(|_| {
            EntryIdentityError::new("Windows returned a non-Unicode volume identity".to_owned())
        })?
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    if !is_valid_volume_id(&value) {
        return Err(EntryIdentityError::new(format!(
            "Windows returned an invalid volume identity: {value}"
        )));
    }
    Ok(value)
}

fn null_terminated(value: &OsStr) -> Result<Vec<u16>, EntryIdentityError> {
    let mut encoded: Vec<u16> = value.encode_wide().collect();
    if encoded.contains(&0) {
        return Err(EntryIdentityError::new(
            "project entry path contains a null character".to_owned(),
        ));
    }
    encoded.push(0);
    Ok(encoded)
}

fn last_os_error(action: &str, path: &Path) -> EntryIdentityError {
    EntryIdentityError::new(format!(
        "{action} '{}': {}",
        path.display(),
        std::io::Error::last_os_error()
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryIdentityError {
    message: String,
}

impl EntryIdentityError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for EntryIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for EntryIdentityError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn fixture_path(name: &str) -> PathBuf {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "swawkit-entry-identity-{}-{sequence}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn reads_stable_identity_and_distinguishes_a_copy() {
        let original = fixture_path("original.exe");
        let copy = fixture_path("copy.exe");
        fs::write(&original, "original").expect("write original");
        fs::copy(&original, &copy).expect("copy entry");

        let first = EntryIdentity::read(&original).expect("first identity");
        let second = EntryIdentity::read(&original).expect("second identity");
        let copied = EntryIdentity::read(&copy).expect("copied identity");

        assert_eq!(first, second);
        assert_eq!(first.volume_id(), copied.volume_id());
        assert_ne!(first.file_id(), copied.file_id());
        assert_eq!(first.file_id().len(), 32);

        fs::remove_file(original).expect("remove original");
        fs::remove_file(copy).expect("remove copy");
    }

    #[test]
    fn reads_stable_directory_identity_and_distinguishes_a_replacement() {
        let original = fixture_path("original-directory");
        let displaced = fixture_path("displaced-directory");
        fs::create_dir(&original).expect("create original directory");

        let first = EntryIdentity::read_directory(&original).expect("first identity");
        fs::rename(&original, &displaced).expect("displace original directory");
        let moved = EntryIdentity::read_directory(&displaced).expect("moved identity");
        fs::create_dir(&original).expect("create replacement directory");
        let replacement = EntryIdentity::read_directory(&original).expect("replacement identity");

        assert_eq!(first, moved);
        assert_eq!(first.volume_id(), replacement.volume_id());
        assert_ne!(first.file_id(), replacement.file_id());

        fs::remove_dir(original).expect("remove replacement directory");
        fs::remove_dir(displaced).expect("remove displaced directory");
    }

    #[test]
    fn validates_the_persisted_v0_identity_shape() {
        assert!(EntryIdentity::from_parts(
            r"\\?\volume{91cf565a-694f-4232-be2d-368578d28629}",
            "0000000000000000001400000000685d"
        )
        .is_ok());
        assert!(EntryIdentity::from_parts("D:", "ABCDEF0123456789").is_err());
        assert!(EntryIdentity::from_parts(
            r"\\?\volume{91cf565a-694f-4232-be2d-368578d28629}",
            "ABCDEF0123456789"
        )
        .is_err());
    }
}
