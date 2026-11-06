use crate::state::PickedSource;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Symbolic,
    RelativeSymbolic,
    Junction,
    HardLink,
}

impl LinkKind {
    pub const fn cli_name(&self) -> &'static str {
        match self {
            Self::Symbolic => "symbolic",
            Self::RelativeSymbolic => "relative-symbolic",
            Self::Junction => "junction",
            Self::HardLink => "hardlink",
        }
    }

    pub fn from_cli_name(name: &str) -> Option<Self> {
        match name {
            "symbolic" => Some(Self::Symbolic),
            "relative-symbolic" => Some(Self::RelativeSymbolic),
            "junction" => Some(Self::Junction),
            "hardlink" => Some(Self::HardLink),
            _ => None,
        }
    }

    pub const fn drop_subcommand(&self) -> &'static str {
        match self {
            Self::Symbolic => "drop-symlink",
            Self::RelativeSymbolic => "drop-relative-symlink",
            Self::Junction => "drop-junction",
            Self::HardLink => "drop-hardlink",
        }
    }

    pub fn from_drop_subcommand(name: &str) -> Option<Self> {
        match name {
            "drop-symlink" => Some(Self::Symbolic),
            "drop-relative-symlink" => Some(Self::RelativeSymbolic),
            "drop-junction" => Some(Self::Junction),
            "drop-hardlink" => Some(Self::HardLink),
            _ => None,
        }
    }

    pub const fn drop_title(&self) -> &'static str {
        match self {
            Self::Symbolic => "Drop Symbolic Link",
            Self::RelativeSymbolic => "Drop Relative Symbolic Link",
            Self::Junction => "Drop Directory Junction",
            Self::HardLink => "Drop Hard Link",
        }
    }

    pub const fn create_title(&self) -> &'static str {
        match self {
            Self::Symbolic => "Create Symbolic Links...",
            Self::RelativeSymbolic => "Create Relative Symbolic Links...",
            Self::Junction => "Create Directory Junctions...",
            Self::HardLink => "Create Hard Links...",
        }
    }

    pub fn supports_sources(&self, sources: &[PickedSource]) -> bool {
        match self {
            Self::Symbolic | Self::RelativeSymbolic => true,
            Self::Junction => !sources.is_empty() && sources.iter().all(|s| s.is_dir),
            Self::HardLink => !sources.is_empty() && sources.iter().all(|s| !s.is_dir),
        }
    }
}

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("junctions can only target directories: {0}")]
    JunctionNeedsDirectory(PathBuf),
    #[error("hard links can only target files: {0}")]
    HardLinkNeedsFile(PathBuf),
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
    #[error("cannot create relative link across different drives: from {0} to {1}")]
    RelativePathFailed(PathBuf, PathBuf),
}

pub fn is_elevation_required(error: &LinkError) -> bool {
    match error {
        LinkError::CreateFailed { error, .. } => {
            matches!(error.raw_os_error(), Some(5) | Some(1314))
        }
        _ => false,
    }
}

#[derive(Debug, Error)]
pub enum BatchLinkError {
    #[error("selected link kind does not support the given sources")]
    UnsupportedSources,
    #[error("no source paths were provided")]
    EmptySources,
    #[error("link operation failed: {error}")]
    Failed {
        error: LinkError,
        remaining_sources: Vec<PathBuf>,
    },
}

impl BatchLinkError {
    pub fn is_elevation_required(&self) -> bool {
        match self {
            Self::Failed { error, .. } => is_elevation_required(error),
            _ => false,
        }
    }
}

pub fn execute_batch(
    kind: LinkKind,
    target_dir: &Path,
    sources: &[PickedSource],
) -> Result<(), BatchLinkError> {
    if sources.is_empty() {
        return Err(BatchLinkError::EmptySources);
    }
    if !kind.supports_sources(sources) {
        return Err(BatchLinkError::UnsupportedSources);
    }
    for (index, source) in sources.iter().enumerate() {
        if let Err(err) = create_link(kind, source, target_dir) {
            let remaining = sources[index..].iter().map(|s| s.path.clone()).collect();
            return Err(BatchLinkError::Failed {
                error: err,
                remaining_sources: remaining,
            });
        }
    }
    Ok(())
}

fn source_file_name(source: &Path) -> Option<&std::ffi::OsStr> {
    #[cfg(windows)]
    {
        source.file_name()
    }
    #[cfg(not(windows))]
    {
        source.file_name().and_then(|name| {
            if let Some(s) = name.to_str() {
                if let Some(pos) = s.rfind('\\') {
                    return Some(std::ffi::OsStr::new(&s[pos + 1..]));
                }
            }
            Some(name)
        })
    }
}

pub fn destination_for(target_dir: &Path, source: &Path) -> Result<PathBuf, LinkError> {
    let name = source_file_name(source)
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
    if kind == LinkKind::HardLink && source.is_dir {
        return Err(LinkError::HardLinkNeedsFile(source.path.clone()));
    }

    if !target_dir.is_dir() {
        return Err(LinkError::MissingTargetDirectory(target_dir.to_path_buf()));
    }

    let mut destination = destination_for(target_dir, &source.path)?;
    if fs::symlink_metadata(&destination).is_ok() {
        let name = source.path.file_name().unwrap();
        let stem = source.path.file_stem().unwrap_or(name).to_string_lossy();
        let ext = source.path.extension().map(|e| e.to_string_lossy().into_owned());
        
        let mut counter = 1;
        loop {
            let new_name = match &ext {
                Some(e) => format!("{} ({}).{}", stem, counter, e),
                None => format!("{} ({})", stem, counter),
            };
            destination = target_dir.join(new_name);
            if fs::symlink_metadata(&destination).is_err() {
                break;
            }
            counter += 1;
        }
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
    let source_path = match kind {
        LinkKind::RelativeSymbolic => {
            let target_dir = destination.parent().unwrap_or_else(|| Path::new(""));
            pathdiff::diff_paths(&source.path, target_dir)
                .ok_or_else(|| LinkError::RelativePathFailed(source.path.clone(), target_dir.to_path_buf()))?
        }
        _ => source.path.clone(),
    };

    match kind {
        LinkKind::Symbolic | LinkKind::RelativeSymbolic => {
            let result = if source.is_dir {
                std::os::windows::fs::symlink_dir(&source_path, destination)
            } else {
                std::os::windows::fs::symlink_file(&source_path, destination)
            };
            result.map_err(|error| LinkError::CreateFailed {
                source: source.path.clone(),
                destination: destination.to_path_buf(),
                error,
            })
        }
        LinkKind::Junction => {
            junction::create(&source.path, destination).map_err(|error| LinkError::CreateFailed {
                source: source.path.clone(),
                destination: destination.to_path_buf(),
                error,
            })
        }
        LinkKind::HardLink => {
            fs::hard_link(&source.path, destination).map_err(|error| LinkError::CreateFailed {
                source: source.path.clone(),
                destination: destination.to_path_buf(),
                error,
            })
        }
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
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
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
        let print = source
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();

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

        buffer.reparse_data_length =
            8 + buffer.substitute_name_length + 2 + buffer.print_name_length + 2;
        let total_size = 8 + buffer.reparse_data_length as u32;
        if total_size > MAXIMUM_REPARSE_DATA_BUFFER_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "junction target is too long",
            ));
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
        let native = native_target_text(&text);
        Ok(OsStr::new(&native).encode_wide().chain(Some(0)).collect())
    }

    fn native_target_text(path: &str) -> String {
        if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
            format!(r"\??\UNC\{}", rest)
        } else if let Some(rest) = path.strip_prefix(r"\\?\") {
            format!(r"\??\{}", rest)
        } else if let Some(rest) = path.strip_prefix(r"\\") {
            format!(r"\??\UNC\{}", rest)
        } else {
            format!(r"\??\{}", path)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn native_target_uses_nt_prefix() {
            let wide = native_target_for_test(r"C:\Target");
            let text = String::from_utf16_lossy(&wide);
            assert!(text.starts_with(r"\??\"));
        }

        #[test]
        fn native_target_converts_extended_drive_prefix() {
            assert_eq!(native_target_text(r"\\?\D:\Target"), r"\??\D:\Target");
        }

        #[test]
        fn native_target_converts_extended_unc_prefix() {
            assert_eq!(
                native_target_text(r"\\?\UNC\server\share\Target"),
                r"\??\UNC\server\share\Target"
            );
        }

        #[test]
        fn native_target_converts_unc_prefix() {
            assert_eq!(
                native_target_text(r"\\server\share\Target"),
                r"\??\UNC\server\share\Target"
            );
        }

        fn native_target_for_test(path: &str) -> Vec<u16> {
            OsStr::new(&native_target_text(path))
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
        assert_eq!(dest, Path::new(r"C:\out").join("file.txt"));
    }

    #[test]
    fn link_kind_conversions_and_titles() {
        for kind in [
            LinkKind::Symbolic,
            LinkKind::RelativeSymbolic,
            LinkKind::Junction,
            LinkKind::HardLink,
        ] {
            assert_eq!(LinkKind::from_cli_name(kind.cli_name()), Some(kind));
            assert_eq!(LinkKind::from_drop_subcommand(kind.drop_subcommand()), Some(kind));
            assert!(!kind.drop_title().is_empty());
            assert!(!kind.create_title().is_empty());
        }
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

    #[test]
    fn hard_links_reject_directories() {
        let source = PickedSource {
            path: PathBuf::from(r"C:\folder"),
            is_dir: true,
        };
        let err = create_link(LinkKind::HardLink, &source, Path::new(r"C:\missing")).unwrap_err();
        assert!(matches!(err, LinkError::HardLinkNeedsFile(_)));
    }

    #[test]
    fn symbolic_links_allow_files_and_directories() {
        let file = PickedSource {
            path: PathBuf::from(r"C:\src\file.txt"),
            is_dir: false,
        };
        let dir = PickedSource {
            path: PathBuf::from(r"C:\src\folder"),
            is_dir: true,
        };
        assert!(matches!(
            create_link(LinkKind::Symbolic, &file, Path::new(r"C:\missing")),
            Err(LinkError::MissingTargetDirectory(_))
        ));
        assert!(matches!(
            create_link(LinkKind::Symbolic, &dir, Path::new(r"C:\missing")),
            Err(LinkError::MissingTargetDirectory(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn junction_links_to_directory_target() {
        let root = std::env::temp_dir().join(format!(
            "lsenext-junction-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&target_dir).unwrap();

        let source = PickedSource {
            path: source_dir.clone(),
            is_dir: true,
        };
        let destination = create_link(LinkKind::Junction, &source, &target_dir).unwrap();

        assert!(destination.is_dir());
        assert_eq!(
            destination.canonicalize().unwrap(),
            source_dir.canonicalize().unwrap()
        );

        let _ = fs::remove_dir(&destination);
        let _ = fs::remove_dir_all(&root);
    }
}
