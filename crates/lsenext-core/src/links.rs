use crate::state::PickedSource;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Symbolic,
    Junction,
}

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("junctions can only target directories: {0}")]
    JunctionNeedsDirectory(PathBuf),
    #[error("target directory does not exist: {0}")]
    MissingTargetDirectory(PathBuf),
    #[error("destination already exists: {0}")]
    DestinationExists(PathBuf),
    #[error("source path has no file name: {0}")]
    MissingFileName(PathBuf),
    #[error("failed to create link from {source} to {destination}: {error}")]
    CreateFailed {
        source: PathBuf,
        destination: PathBuf,
        #[source]
        error: std::io::Error,
    },
}

pub fn destination_for(target_dir: &Path, source: &Path) -> Result<PathBuf, LinkError> {
    let name = source
        .file_name()
        .ok_or_else(|| LinkError::MissingFileName(source.to_path_buf()))?;
    Ok(target_dir.join(name))
}

pub fn create_link(
    kind: LinkKind,
    source: &PickedSource,
    target_dir: &Path,
) -> Result<PathBuf, LinkError> {
    if kind == LinkKind::Junction && !source.is_dir {
        return Err(LinkError::JunctionNeedsDirectory(source.path.clone()));
    }

    if !target_dir.is_dir() {
        return Err(LinkError::MissingTargetDirectory(target_dir.to_path_buf()));
    }

    let destination = destination_for(target_dir, &source.path)?;
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(LinkError::DestinationExists(destination));
    }

    create_platform_link(kind, source, &destination)?;
    Ok(destination)
}

#[cfg(windows)]
fn create_platform_link(
    kind: LinkKind,
    source: &PickedSource,
    destination: &Path,
) -> Result<(), LinkError> {
    match kind {
        LinkKind::Symbolic => {
            let result = if source.is_dir {
                std::os::windows::fs::symlink_dir(&source.path, destination)
            } else {
                std::os::windows::fs::symlink_file(&source.path, destination)
            };
            result.map_err(|error| LinkError::CreateFailed {
                source: source.path.clone(),
                destination: destination.to_path_buf(),
                error,
            })
        }
        LinkKind::Junction => junction::create(&source.path, destination).map_err(|error| {
            LinkError::CreateFailed {
                source: source.path.clone(),
                destination: destination.to_path_buf(),
                error,
            }
        }),
    }
}

#[cfg(not(windows))]
fn create_platform_link(
    _kind: LinkKind,
    source: &PickedSource,
    destination: &Path,
) -> Result<(), LinkError> {
    Err(LinkError::CreateFailed {
        source: source.path.clone(),
        destination: destination.to_path_buf(),
        error: std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "LSENext link creation is only supported on Windows",
        ),
    })
}

#[cfg(windows)]
mod junction {
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::mem::zeroed;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE,
        MAXIMUM_REPARSE_DATA_BUFFER_SIZE,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;
    const FSCTL_SET_REPARSE_POINT_LOCAL: u32 = 0x000900A4;

    #[repr(C)]
    struct ReparseDataBuffer {
        reparse_tag: u32,
        reparse_data_length: u16,
        reserved: u16,
        substitute_name_offset: u16,
        substitute_name_length: u16,
        print_name_offset: u16,
        print_name_length: u16,
        path_buffer: [u16; 0x3ff0],
    }

    pub fn create(source: &Path, destination: &Path) -> io::Result<()> {
        fs::create_dir(destination)?;
        let result = set_mount_point(source, destination);
        if result.is_err() {
            let _ = fs::remove_dir(destination);
        }
        result
    }

    fn set_mount_point(source: &Path, destination: &Path) -> io::Result<()> {
        let substitute = native_target(source)?;
        let print = source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();

        let mut buffer: ReparseDataBuffer = unsafe { zeroed() };
        buffer.reparse_tag = IO_REPARSE_TAG_MOUNT_POINT;
        buffer.substitute_name_offset = 0;
        buffer.substitute_name_length = ((substitute.len() - 1) * 2) as u16;
        buffer.print_name_offset = buffer.substitute_name_length + 2;
        buffer.print_name_length = ((print.len() - 1) * 2) as u16;

        let mut cursor = 0usize;
        buffer.path_buffer[cursor..cursor + substitute.len()].copy_from_slice(&substitute);
        cursor += substitute.len();
        buffer.path_buffer[cursor..cursor + print.len()].copy_from_slice(&print);

        buffer.reparse_data_length = 8 + buffer.substitute_name_length + 2 + buffer.print_name_length + 2;
        let total_size = 8 + buffer.reparse_data_length as u32;
        if total_size > MAXIMUM_REPARSE_DATA_BUFFER_SIZE {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "junction target is too long"));
        }

        let file = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .access_mode(FILE_GENERIC_WRITE)
            .open(destination)?;

        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle() as HANDLE,
                FSCTL_SET_REPARSE_POINT_LOCAL,
                &mut buffer as *mut _ as *mut _,
                total_size,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn native_target(source: &Path) -> io::Result<Vec<u16>> {
        let canonical = source.canonicalize()?;
        let text = canonical.to_string_lossy();
        let native = if text.starts_with(r"\\?\") {
            text.to_string()
        } else if text.starts_with(r"\\") {
            format!(r"\??\UNC\{}", text.trim_start_matches(r"\\"))
        } else {
            format!(r"\??\{}", text)
        };
        Ok(OsStr::new(&native).encode_wide().chain(Some(0)).collect())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn native_target_uses_nt_prefix() {
            let path = Path::new(r"C:\Target");
            let wide = native_target_for_test(path);
            let text = String::from_utf16_lossy(&wide);
            assert!(text.starts_with(r"\??\"));
        }

        fn native_target_for_test(path: &Path) -> Vec<u16> {
            let text = path.to_string_lossy();
            OsStr::new(&format!(r"\??\{}", text))
                .encode_wide()
                .chain(Some(0))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_uses_source_file_name() {
        let dest = destination_for(Path::new(r"C:\out"), Path::new(r"C:\in\file.txt")).unwrap();
        assert_eq!(dest, PathBuf::from(r"C:\out\file.txt"));
    }

    #[test]
    fn junction_rejects_files() {
        let source = PickedSource {
            path: PathBuf::from(r"C:\file.txt"),
            is_dir: false,
        };
        let err = create_link(LinkKind::Junction, &source, Path::new(r"C:\missing")).unwrap_err();
        assert!(matches!(err, LinkError::JunctionNeedsDirectory(_)));
    }
}
