#![allow(non_snake_case)]

use lsenext_core::{
    clear_state, elevate_create, execute_batch, load_state, save_sources, show_error,
    BatchLinkError, LinkKind, PickedSource, SelectionState,
};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
#[cfg(feature = "diagnostics")]
use std::process::Command;
use std::sync::OnceLock;
use windows::core::{implement, Interface, GUID, HRESULT, PCSTR, PWSTR};
use windows::Win32::Foundation::{
    BOOL, CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_NOTIMPL, E_OUTOFMEMORY,
    E_POINTER, HINSTANCE, HMODULE, S_FALSE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemAlloc, CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl,
    CLSCTX_INPROC_SERVER,
};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW};
use windows::Win32::UI::Shell::{
    FOS_PICKFOLDERS, IEnumExplorerCommand, IEnumExplorerCommand_Impl, IExplorerCommand,
    IExplorerCommand_Impl, IFileOpenDialog, IShellItemArray, ECF_DEFAULT, ECF_HASSUBCOMMANDS,
    ECS_DISABLED, ECS_ENABLED, ECS_HIDDEN, SIGDN_FILESYSPATH,
};

pub const CLSID_LSENEXT_FILE_ROOT: GUID = GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e000);
pub const CLSID_LSENEXT_PICK_SOURCE: GUID = GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e001);
pub const CLSID_LSENEXT_DROP_SYMLINK: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e002);
pub const CLSID_LSENEXT_DROP_RELATIVE_SYMLINK: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e009);
pub const CLSID_LSENEXT_DROP_JUNCTION: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e003);
pub const CLSID_LSENEXT_CLEAR_SOURCE: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e004);
pub const CLSID_LSENEXT_DIRECTORY_ROOT: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e005);
pub const CLSID_LSENEXT_BACKGROUND_ROOT: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e006);
pub const CLSID_LSENEXT_DIAGNOSTICS: GUID = GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e007);
pub const CLSID_LSENEXT_DROP_HARDLINK: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e008);
pub const CLSID_LSENEXT_CREATE_SYMBOLIC: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e00a);
pub const CLSID_LSENEXT_CREATE_RELATIVE_SYMBOLIC: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e00b);
pub const CLSID_LSENEXT_CREATE_JUNCTION: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e00c);
pub const CLSID_LSENEXT_CREATE_HARDLINK: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e00d);

static MODULE_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootKind {
    File,
    Directory,
    Background,
}

impl RootKind {
    pub const fn clsid(&self) -> GUID {
        match self {
            Self::File => CLSID_LSENEXT_FILE_ROOT,
            Self::Directory => CLSID_LSENEXT_DIRECTORY_ROOT,
            Self::Background => CLSID_LSENEXT_BACKGROUND_ROOT,
        }
    }

    pub fn from_clsid(guid: &GUID) -> Option<Self> {
        match *guid {
            CLSID_LSENEXT_FILE_ROOT => Some(Self::File),
            CLSID_LSENEXT_DIRECTORY_ROOT => Some(Self::Directory),
            CLSID_LSENEXT_BACKGROUND_ROOT => Some(Self::Background),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    PickSource,
    DropSymbolic,
    DropRelativeSymbolic,
    DropJunction,
    DropHardLink,
    CreateSymbolic,
    CreateRelativeSymbolic,
    CreateJunction,
    CreateHardLink,
    ClearSource,
    #[cfg(feature = "diagnostics")]
    Diagnostics,
}

impl CommandKind {
    pub const fn clsid(&self) -> GUID {
        match self {
            Self::PickSource => CLSID_LSENEXT_PICK_SOURCE,
            Self::DropSymbolic => CLSID_LSENEXT_DROP_SYMLINK,
            Self::DropRelativeSymbolic => CLSID_LSENEXT_DROP_RELATIVE_SYMLINK,
            Self::DropJunction => CLSID_LSENEXT_DROP_JUNCTION,
            Self::DropHardLink => CLSID_LSENEXT_DROP_HARDLINK,
            Self::CreateSymbolic => CLSID_LSENEXT_CREATE_SYMBOLIC,
            Self::CreateRelativeSymbolic => CLSID_LSENEXT_CREATE_RELATIVE_SYMBOLIC,
            Self::CreateJunction => CLSID_LSENEXT_CREATE_JUNCTION,
            Self::CreateHardLink => CLSID_LSENEXT_CREATE_HARDLINK,
            Self::ClearSource => CLSID_LSENEXT_CLEAR_SOURCE,
            #[cfg(feature = "diagnostics")]
            Self::Diagnostics => CLSID_LSENEXT_DIAGNOSTICS,
        }
    }

    pub fn from_clsid(guid: &GUID) -> Option<Self> {
        match *guid {
            CLSID_LSENEXT_PICK_SOURCE => Some(Self::PickSource),
            CLSID_LSENEXT_DROP_SYMLINK => Some(Self::DropSymbolic),
            CLSID_LSENEXT_DROP_RELATIVE_SYMLINK => Some(Self::DropRelativeSymbolic),
            CLSID_LSENEXT_DROP_JUNCTION => Some(Self::DropJunction),
            CLSID_LSENEXT_DROP_HARDLINK => Some(Self::DropHardLink),
            CLSID_LSENEXT_CREATE_SYMBOLIC => Some(Self::CreateSymbolic),
            CLSID_LSENEXT_CREATE_RELATIVE_SYMBOLIC => Some(Self::CreateRelativeSymbolic),
            CLSID_LSENEXT_CREATE_JUNCTION => Some(Self::CreateJunction),
            CLSID_LSENEXT_CREATE_HARDLINK => Some(Self::CreateHardLink),
            CLSID_LSENEXT_CLEAR_SOURCE => Some(Self::ClearSource),
            #[cfg(feature = "diagnostics")]
            CLSID_LSENEXT_DIAGNOSTICS => Some(Self::Diagnostics),
            _ => None,
        }
    }

    pub const fn title(&self) -> &'static str {
        match self {
            Self::PickSource => "Pick Link Source",
            Self::DropSymbolic => LinkKind::Symbolic.drop_title(),
            Self::DropRelativeSymbolic => LinkKind::RelativeSymbolic.drop_title(),
            Self::DropJunction => LinkKind::Junction.drop_title(),
            Self::DropHardLink => LinkKind::HardLink.drop_title(),
            Self::CreateSymbolic => LinkKind::Symbolic.create_title(),
            Self::CreateRelativeSymbolic => LinkKind::RelativeSymbolic.create_title(),
            Self::CreateJunction => LinkKind::Junction.create_title(),
            Self::CreateHardLink => LinkKind::HardLink.create_title(),
            Self::ClearSource => "Clear Link Source",
            #[cfg(feature = "diagnostics")]
            Self::Diagnostics => "Debug Diagnostics",
        }
    }
}

#[implement(IExplorerCommand)]
struct RootCommand {
    kind: RootKind,
}

#[implement(IExplorerCommand)]
struct MenuCommand {
    kind: CommandKind,
}

#[implement(IEnumExplorerCommand)]
struct CommandEnum {
    commands: Vec<IExplorerCommand>,
    index: std::cell::Cell<usize>,
}

impl IExplorerCommand_Impl for RootCommand_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        alloc_pwstr("LSENext")
    }

    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        icon_path()
            .and_then(|path| path.to_str().map(alloc_pwstr))
            .unwrap_or_else(|| Err(E_NOTIMPL.into()))
    }

    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        alloc_pwstr("LSENext")
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        Ok(self.kind.clsid())
    }

    fn GetState(
        &self,
        _items: Option<&IShellItemArray>,
        _ok_to_be_slow: BOOL,
    ) -> windows::core::Result<u32> {
        Ok(ECS_ENABLED.0 as u32)
    }

    fn Invoke(
        &self,
        _items: Option<&IShellItemArray>,
        _bind_ctx: Option<&IBindCtx>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn GetFlags(&self) -> windows::core::Result<u32> {
        Ok((ECF_DEFAULT.0 | ECF_HASSUBCOMMANDS.0) as u32)
    }

    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        enumerate_commands(self.kind)
    }
}

impl IExplorerCommand_Impl for MenuCommand_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        alloc_pwstr(self.kind.title())
    }

    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        icon_path()
            .and_then(|path| path.to_str().map(alloc_pwstr))
            .unwrap_or_else(|| Err(E_NOTIMPL.into()))
    }

    fn GetToolTip(&self, items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        self.GetTitle(items)
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        Ok(self.kind.clsid())
    }

    fn GetState(
        &self,
        items: Option<&IShellItemArray>,
        _ok_to_be_slow: BOOL,
    ) -> windows::core::Result<u32> {
        let state = match self.kind {
            CommandKind::PickSource => ECS_ENABLED,
            #[cfg(feature = "diagnostics")]
            CommandKind::Diagnostics => ECS_ENABLED,
            CommandKind::DropSymbolic | CommandKind::DropRelativeSymbolic | CommandKind::ClearSource => {
                if load_state().ok().flatten().is_some() {
                    ECS_ENABLED
                } else {
                    ECS_DISABLED
                }
            }
            CommandKind::DropJunction => {
                if load_state()
                    .ok()
                    .flatten()
                    .map(|state| LinkKind::Junction.supports_sources(&state.sources))
                    .unwrap_or(false)
                {
                    ECS_ENABLED
                } else {
                    ECS_HIDDEN
                }
            }
            CommandKind::DropHardLink => {
                if load_state()
                    .ok()
                    .flatten()
                    .map(|state| LinkKind::HardLink.supports_sources(&state.sources))
                    .unwrap_or(false)
                {
                    ECS_ENABLED
                } else {
                    ECS_HIDDEN
                }
            }
            CommandKind::CreateSymbolic | CommandKind::CreateRelativeSymbolic => ECS_ENABLED,
            CommandKind::CreateJunction => {
                if items_all_dirs(items) {
                    ECS_ENABLED
                } else {
                    ECS_HIDDEN
                }
            }
            CommandKind::CreateHardLink => {
                if items_all_files(items) {
                    ECS_ENABLED
                } else {
                    ECS_HIDDEN
                }
            }
        };
        Ok(state.0 as u32)
    }

    fn Invoke(
        &self,
        items: Option<&IShellItemArray>,
        _bind_ctx: Option<&IBindCtx>,
    ) -> windows::core::Result<()> {
        let result: Result<(), String> = match self.kind {
            CommandKind::PickSource => pick_sources(items),
            CommandKind::DropSymbolic => drop_links(items, LinkKind::Symbolic),
            CommandKind::DropRelativeSymbolic => drop_links(items, LinkKind::RelativeSymbolic),
            CommandKind::DropJunction => drop_links(items, LinkKind::Junction),
            CommandKind::DropHardLink => drop_links(items, LinkKind::HardLink),
            CommandKind::CreateSymbolic => create_links_from_selection(items, LinkKind::Symbolic),
            CommandKind::CreateRelativeSymbolic => {
                create_links_from_selection(items, LinkKind::RelativeSymbolic)
            }
            CommandKind::CreateJunction => {
                create_links_from_selection(items, LinkKind::Junction)
            }
            CommandKind::CreateHardLink => {
                create_links_from_selection(items, LinkKind::HardLink)
            }
            CommandKind::ClearSource => clear_state().map(|_| ()).map_err(|err| err.to_string()),
            #[cfg(feature = "diagnostics")]
            CommandKind::Diagnostics => {
                run_helper_command("diagnostics").map_err(|err| err.to_string())
            }
        };

        result.map_err(|err| {
            show_error(&err);
            E_FAIL.into()
        })
    }

    fn GetFlags(&self) -> windows::core::Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }

    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

impl IEnumExplorerCommand_Impl for CommandEnum_Impl {
    fn Next(
        &self,
        celt: u32,
        puicommand: *mut Option<IExplorerCommand>,
        pceltfetched: *mut u32,
    ) -> HRESULT {
        if puicommand.is_null() {
            return E_POINTER;
        }

        let start = self.index.get();
        let remaining = self.commands.len().saturating_sub(start);
        let take = remaining.min(celt as usize);

        unsafe {
            for slot in 0..take {
                *puicommand.add(slot) = Some(self.commands[start + slot].clone());
            }
            for slot in take..celt as usize {
                *puicommand.add(slot) = None;
            }
            if !pceltfetched.is_null() {
                *pceltfetched = take as u32;
            }
        }

        self.index.set(start + take);
        if take == celt as usize {
            windows::core::HRESULT(0)
        } else {
            S_FALSE
        }
    }

    fn Skip(&self, celt: u32) -> windows::core::Result<()> {
        let next = self.index.get().saturating_add(celt as usize);
        if next <= self.commands.len() {
            self.index.set(next);
            Ok(())
        } else {
            Err(E_FAIL.into())
        }
    }

    fn Reset(&self) -> windows::core::Result<()> {
        self.index.set(0);
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IEnumExplorerCommand> {
        Ok(CommandEnum {
            commands: self.commands.clone(),
            index: std::cell::Cell::new(self.index.get()),
        }
        .into())
    }
}

#[implement(IClassFactory)]
struct ClassFactory {
    kind: FactoryKind,
}

#[derive(Clone, Copy)]
enum FactoryKind {
    Root(RootKind),
    Menu(CommandKind),
}

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Option<&windows::core::IUnknown>,
        riid: *const GUID,
        object: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        if object.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *object = std::ptr::null_mut();
        }

        let command: IExplorerCommand = match self.kind {
            FactoryKind::Root(kind) => RootCommand { kind }.into(),
            FactoryKind::Menu(kind) => MenuCommand { kind }.into(),
        };
        unsafe {
            command.query(riid, object).ok()?;
        }
        Ok(())
    }

    fn LockServer(&self, _lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

#[no_mangle]
pub unsafe extern "system" fn DllMain(
    module: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    if reason == 1 {
        let _ = DisableThreadLibraryCalls(module);
        if let Some(path) = module_path(module) {
            let _ = MODULE_PATH.set(path);
        }
    }
    BOOL(1)
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

#[no_mangle]
pub extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || object.is_null() {
        return E_POINTER;
    }
    unsafe {
        *object = std::ptr::null_mut();
    }

    let kind = unsafe {
        if let Some(root) = RootKind::from_clsid(&*rclsid) {
            FactoryKind::Root(root)
        } else if let Some(cmd) = CommandKind::from_clsid(&*rclsid) {
            FactoryKind::Menu(cmd)
        } else {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
    };

    let factory: IClassFactory = ClassFactory { kind }.into();
    unsafe { factory.query(riid, object).ok().into() }
}

#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    E_NOTIMPL
}

#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    E_NOTIMPL
}

#[no_mangle]
pub extern "system" fn LSENextVersion() -> PCSTR {
    PCSTR(concat!("LSENext ", env!("CARGO_PKG_VERSION"), "\0").as_ptr())
}

fn enumerate_commands(root_kind: RootKind) -> windows::core::Result<IEnumExplorerCommand> {
    let commands = menu_command_kinds(root_kind, load_state().ok().flatten())
        .into_iter()
        .map(|kind| MenuCommand { kind }.into())
        .collect();

    Ok(CommandEnum {
        commands,
        index: std::cell::Cell::new(0),
    }
    .into())
}

pub fn menu_command_kinds(root_kind: RootKind, state: Option<SelectionState>) -> Vec<CommandKind> {
    let mut commands = match root_kind {
        RootKind::File | RootKind::Directory => vec![CommandKind::PickSource],
        RootKind::Background => Vec::new(),
    };
    if root_kind == RootKind::File {
        commands.extend([
            CommandKind::CreateSymbolic,
            CommandKind::CreateRelativeSymbolic,
            CommandKind::CreateJunction,
            CommandKind::CreateHardLink,
        ]);
        #[cfg(feature = "diagnostics")]
        commands.push(CommandKind::Diagnostics);
        return commands;
    }
    if let Some(state) = state {
        commands.push(CommandKind::DropSymbolic);
        commands.push(CommandKind::DropRelativeSymbolic);
        if LinkKind::Junction.supports_sources(&state.sources) {
            commands.push(CommandKind::DropJunction);
        }
        if LinkKind::HardLink.supports_sources(&state.sources) {
            commands.push(CommandKind::DropHardLink);
        }
        commands.push(CommandKind::ClearSource);
    }
    #[cfg(feature = "diagnostics")]
    commands.push(CommandKind::Diagnostics);
    commands
}

#[cfg(feature = "diagnostics")]
fn run_helper_command(command: &str) -> Result<(), std::io::Error> {
    let helper = helper_path().unwrap_or_else(|| PathBuf::from("lsenext-helper.exe"));
    Command::new(helper).arg(command).spawn()?;
    Ok(())
}

fn helper_path() -> Option<PathBuf> {
    MODULE_PATH.get().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("lsenext-helper.exe"))
    })
}

fn icon_path() -> Option<PathBuf> {
    MODULE_PATH.get().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("Assets\\LSENext.ico"))
    })
}

fn module_path(module: HINSTANCE) -> Option<PathBuf> {
    let mut buffer = [0u16; 32768];
    let module = HMODULE(module.0);
    let len = unsafe { GetModuleFileNameW(Some(&module), &mut buffer) };
    if len == 0 {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(
        &buffer[..len as usize],
    )))
}

fn pick_sources(items: Option<&IShellItemArray>) -> Result<(), String> {
    let paths = shell_item_paths(items).map_err(|err| err.message().to_string())?;
    save_sources(&paths)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn drop_links(items: Option<&IShellItemArray>, kind: LinkKind) -> Result<(), String> {
    let target = shell_item_paths(items)
        .map_err(|err| err.message().to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| "target directory is required".to_string())?;
    let state = load_state()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "Pick a link source before creating a link.".to_string())?;

    if let Err(batch_err) = execute_batch(kind, &target, &state.sources) {
        if batch_err.is_elevation_required() {
            let helper = helper_path().ok_or_else(|| {
                "cannot locate LSENext helper next to the shell extension".to_string()
            })?;
            if let BatchLinkError::Failed {
                remaining_sources, ..
            } = &batch_err
            {
                return elevate_create(&helper, kind, &target, remaining_sources).map_err(|e| {
                    format!("{}\n\nElevated retry failed: {}", batch_err, e)
                });
            }
        }
        return Err(batch_err.to_string());
    }
    Ok(())
}

fn shell_item_paths(items: Option<&IShellItemArray>) -> windows::core::Result<Vec<PathBuf>> {
    let items = items.ok_or_else(|| {
        windows::core::Error::new(E_FAIL, "Explorer did not provide a selected item.")
    })?;
    let count = unsafe { items.GetCount() }?;
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let item = unsafe { items.GetItemAt(index) }?;
        let raw = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }?;
        let path = unsafe { raw.to_string() }?;
        unsafe {
            CoTaskMemFree(Some(raw.as_ptr() as _));
        }
        paths.push(PathBuf::from(path));
    }
    Ok(paths)
}

fn alloc_pwstr(text: &str) -> windows::core::Result<PWSTR> {
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();
    let ptr = unsafe { CoTaskMemAlloc(bytes) as *mut u16 };
    if ptr.is_null() {
        return Err(E_OUTOFMEMORY.into());
    }
    unsafe {
        ptr.copy_from_nonoverlapping(wide.as_ptr(), wide.len());
    }
    Ok(PWSTR::from_raw(ptr))
}

fn items_all_dirs(items: Option<&IShellItemArray>) -> bool {
    let paths = match shell_item_paths(items) {
        Ok(paths) => paths,
        Err(_) => return false,
    };
    !paths.is_empty() && paths.iter().all(|p| p.is_dir())
}

fn items_all_files(items: Option<&IShellItemArray>) -> bool {
    let paths = match shell_item_paths(items) {
        Ok(paths) => paths,
        Err(_) => return false,
    };
    !paths.is_empty() && paths.iter().all(|p| p.is_file())
}

fn pick_folder() -> Result<Option<PathBuf>, String> {
    const CLSID_FILEOPENDIALOG: GUID =
        GUID::from_u128(0xDC1C5A9C_E88A_4dde_A5A1_60F82A20AEF7);

    const HRESULT_CANCELLED: windows::core::HRESULT =
        windows::core::HRESULT(0x800704C7u32 as i32);

    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&CLSID_FILEOPENDIALOG, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("failed to create folder picker: {}", e))?;

        dialog
            .SetOptions(FOS_PICKFOLDERS)
            .map_err(|e| format!("failed to set folder picker options: {}", e))?;

        match dialog.Show(None) {
            Ok(()) => {}
            Err(e) => {
                if e.code() == HRESULT_CANCELLED {
                    return Ok(None);
                }
                return Err(format!("folder picker error: {}", e));
            }
        }

        let item = dialog
            .GetResult()
            .map_err(|e| format!("failed to get selected folder: {}", e))?;

        let name = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|e| format!("failed to get folder path: {}", e))?;

        let path = name
            .to_string()
            .map_err(|e| format!("failed to convert folder path: {}", e))?;
        CoTaskMemFree(Some(name.as_ptr() as _));

        Ok(Some(PathBuf::from(path)))
    }
}

fn create_links_from_selection(
    items: Option<&IShellItemArray>,
    kind: LinkKind,
) -> Result<(), String> {
    let source_paths = shell_item_paths(items).map_err(|err| err.message().to_string())?;
    if source_paths.is_empty() {
        return Err("no files selected".to_string());
    }
    let target = match pick_folder()? {
        Some(target) => target,
        None => return Ok(()),
    };
    let sources: Vec<PickedSource> = source_paths
        .iter()
        .map(|path| PickedSource {
            path: path.clone(),
            is_dir: path.is_dir(),
        })
        .collect();

    if let Err(batch_err) = execute_batch(kind, &target, &sources) {
        if batch_err.is_elevation_required() {
            let helper = helper_path().ok_or_else(|| {
                "cannot locate LSENext helper next to the shell extension".to_string()
            })?;
            if let BatchLinkError::Failed {
                remaining_sources, ..
            } = &batch_err
            {
                return elevate_create(&helper, kind, &target, remaining_sources).map_err(|e| {
                    format!("{}\n\nElevated retry failed: {}", batch_err, e)
                });
            }
        }
        return Err(batch_err.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(unused_mut)]
    fn with_diagnostics(mut commands: Vec<CommandKind>) -> Vec<CommandKind> {
        #[cfg(feature = "diagnostics")]
        commands.push(CommandKind::Diagnostics);
        commands
    }

    #[test]
    fn file_sources_do_not_offer_directory_junctions() {
        let state = SelectionState {
            picked_at_unix: 42,
            sources: vec![PickedSource {
                path: PathBuf::from(r"C:\src\file.txt"),
                is_dir: false,
            }],
        };
        assert_eq!(
            menu_command_kinds(RootKind::Directory, Some(state)),
            with_diagnostics(vec![
                CommandKind::PickSource,
                CommandKind::DropSymbolic,
                CommandKind::DropRelativeSymbolic,
                CommandKind::DropHardLink,
                CommandKind::ClearSource,
            ])
        );
    }

    #[test]
    fn directory_sources_offer_directory_junctions() {
        let state = SelectionState {
            picked_at_unix: 42,
            sources: vec![PickedSource {
                path: PathBuf::from(r"C:\src\folder"),
                is_dir: true,
            }],
        };
        assert_eq!(
            menu_command_kinds(RootKind::Directory, Some(state)),
            with_diagnostics(vec![
                CommandKind::PickSource,
                CommandKind::DropSymbolic,
                CommandKind::DropRelativeSymbolic,
                CommandKind::DropJunction,
                CommandKind::ClearSource,
            ])
        );
    }

    #[test]
    fn no_state_only_shows_pick_source() {
        assert_eq!(
            menu_command_kinds(RootKind::Directory, None),
            with_diagnostics(vec![CommandKind::PickSource])
        );
    }

    #[test]
    fn file_root_shows_pick_source_and_create_links() {
        assert_eq!(
            menu_command_kinds(RootKind::File, None),
            with_diagnostics(vec![
                CommandKind::PickSource,
                CommandKind::CreateSymbolic,
                CommandKind::CreateRelativeSymbolic,
                CommandKind::CreateJunction,
                CommandKind::CreateHardLink,
            ])
        );
    }

    #[test]
    fn background_root_only_shows_drop_commands() {
        let state = SelectionState {
            picked_at_unix: 42,
            sources: vec![PickedSource {
                path: PathBuf::from(r"C:\src\folder"),
                is_dir: true,
            }],
        };
        assert_eq!(
            menu_command_kinds(RootKind::Background, Some(state)),
            with_diagnostics(vec![
                CommandKind::DropSymbolic,
                CommandKind::DropRelativeSymbolic,
                CommandKind::DropJunction,
                CommandKind::ClearSource,
            ])
        );
    }

    #[test]
    fn file_background_sources_offer_hard_links() {
        let state = SelectionState {
            picked_at_unix: 42,
            sources: vec![PickedSource {
                path: PathBuf::from(r"C:\src\file.txt"),
                is_dir: false,
            }],
        };
        assert_eq!(
            menu_command_kinds(RootKind::Background, Some(state)),
            with_diagnostics(vec![
                CommandKind::DropSymbolic,
                CommandKind::DropRelativeSymbolic,
                CommandKind::DropHardLink,
                CommandKind::ClearSource,
            ])
        );
    }

    #[test]
    fn mixed_sources_only_offer_symbolic_links() {
        let state = SelectionState {
            picked_at_unix: 42,
            sources: vec![
                PickedSource {
                    path: PathBuf::from(r"C:\src\file.txt"),
                    is_dir: false,
                },
                PickedSource {
                    path: PathBuf::from(r"C:\src\folder"),
                    is_dir: true,
                },
            ],
        };
        assert_eq!(
            menu_command_kinds(RootKind::Directory, Some(state)),
            with_diagnostics(vec![
                CommandKind::PickSource,
                CommandKind::DropSymbolic,
                CommandKind::DropRelativeSymbolic,
                CommandKind::ClearSource,
            ])
        );
    }
}
