#![allow(non_snake_case)]

use lsenext_core::{clear_state, create_link, load_state, save_sources, LinkKind, SelectionState};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use windows::core::{implement, Interface, GUID, HRESULT, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    BOOL, CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_NOTIMPL, E_OUTOFMEMORY,
    E_POINTER, HINSTANCE, HMODULE, HWND, S_FALSE,
};
use windows::Win32::System::Com::{
    CoTaskMemAlloc, CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl,
};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW};
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl,
    IShellItemArray, ShellExecuteW, ECF_DEFAULT, ECS_DISABLED, ECS_ENABLED, ECS_HIDDEN,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK, SW_SHOWNORMAL};

pub const CLSID_LSENEXT_FILE_ROOT: GUID = GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e000);
pub const CLSID_LSENEXT_PICK_SOURCE: GUID = GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e001);
pub const CLSID_LSENEXT_DROP_SYMLINK: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e002);
pub const CLSID_LSENEXT_DROP_JUNCTION: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e003);
pub const CLSID_LSENEXT_CLEAR_SOURCE: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e004);
pub const CLSID_LSENEXT_DIRECTORY_ROOT: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e005);
pub const CLSID_LSENEXT_BACKGROUND_ROOT: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e006);
pub const CLSID_LSENEXT_DIAGNOSTICS: GUID = GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e007);

static MODULE_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootKind {
    File,
    Directory,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandKind {
    PickSource,
    DropSymbolic,
    DropJunction,
    ClearSource,
    Diagnostics,
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
        Err(E_NOTIMPL.into())
    }

    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        alloc_pwstr("LSENext")
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        Ok(match self.kind {
            RootKind::File => CLSID_LSENEXT_FILE_ROOT,
            RootKind::Directory => CLSID_LSENEXT_DIRECTORY_ROOT,
            RootKind::Background => CLSID_LSENEXT_BACKGROUND_ROOT,
        })
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
        Ok(ECF_DEFAULT.0 as u32 | windows::Win32::UI::Shell::ECF_HASSUBCOMMANDS.0 as u32)
    }

    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        enumerate_commands(self.kind)
    }
}

impl IExplorerCommand_Impl for MenuCommand_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        alloc_pwstr(match self.kind {
            CommandKind::PickSource => "Pick Link Source",
            CommandKind::DropSymbolic => "Drop Symbolic Link",
            CommandKind::DropJunction => "Drop Directory Junction",
            CommandKind::ClearSource => "Clear Link Source",
            CommandKind::Diagnostics => "Debug Diagnostics",
        })
    }

    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        self.GetTitle(_items)
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        Ok(match self.kind {
            CommandKind::PickSource => CLSID_LSENEXT_PICK_SOURCE,
            CommandKind::DropSymbolic => CLSID_LSENEXT_DROP_SYMLINK,
            CommandKind::DropJunction => CLSID_LSENEXT_DROP_JUNCTION,
            CommandKind::ClearSource => CLSID_LSENEXT_CLEAR_SOURCE,
            CommandKind::Diagnostics => CLSID_LSENEXT_DIAGNOSTICS,
        })
    }

    fn GetState(
        &self,
        _items: Option<&IShellItemArray>,
        _ok_to_be_slow: BOOL,
    ) -> windows::core::Result<u32> {
        let state = match self.kind {
            CommandKind::PickSource => ECS_ENABLED.0,
            CommandKind::Diagnostics => ECS_ENABLED.0,
            CommandKind::DropSymbolic | CommandKind::ClearSource => {
                if load_state().ok().flatten().is_some() {
                    ECS_ENABLED.0
                } else {
                    ECS_DISABLED.0
                }
            }
            CommandKind::DropJunction => {
                if load_state()
                    .ok()
                    .flatten()
                    .map(|state| state.sources.iter().all(|source| source.is_dir))
                    .unwrap_or(false)
                {
                    ECS_ENABLED.0
                } else {
                    ECS_HIDDEN.0
                }
            }
        };
        Ok(state as u32)
    }

    fn Invoke(
        &self,
        items: Option<&IShellItemArray>,
        _bind_ctx: Option<&IBindCtx>,
    ) -> windows::core::Result<()> {
        let result: Result<(), String> = match self.kind {
            CommandKind::PickSource => shell_item_paths(items)
                .map_err(|err| err.message().to_string())
                .and_then(|paths| {
                    save_sources(&paths)
                        .map(|_| ())
                        .map_err(|err| err.to_string())
                }),
            CommandKind::DropSymbolic => drop_links(items, LinkKind::Symbolic),
            CommandKind::DropJunction => drop_links(items, LinkKind::Junction),
            CommandKind::ClearSource => clear_state().map(|_| ()).map_err(|err| err.to_string()),
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
            for slot in 0..celt as usize {
                *puicommand.add(slot) = None;
            }
            for slot in 0..take {
                *puicommand.add(slot) = Some(self.commands[start + slot].clone());
            }
        }

        let fetched = take as u32;
        self.index.set(start + take);
        if !pceltfetched.is_null() {
            unsafe {
                *pceltfetched = fetched;
            }
        }

        if fetched == celt {
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
        match *rclsid {
            CLSID_LSENEXT_FILE_ROOT => FactoryKind::Root(RootKind::File),
            CLSID_LSENEXT_DIRECTORY_ROOT => FactoryKind::Root(RootKind::Directory),
            CLSID_LSENEXT_BACKGROUND_ROOT => FactoryKind::Root(RootKind::Background),
            CLSID_LSENEXT_PICK_SOURCE => FactoryKind::Menu(CommandKind::PickSource),
            CLSID_LSENEXT_DROP_SYMLINK => FactoryKind::Menu(CommandKind::DropSymbolic),
            CLSID_LSENEXT_DROP_JUNCTION => FactoryKind::Menu(CommandKind::DropJunction),
            CLSID_LSENEXT_CLEAR_SOURCE => FactoryKind::Menu(CommandKind::ClearSource),
            CLSID_LSENEXT_DIAGNOSTICS => FactoryKind::Menu(CommandKind::Diagnostics),
            _ => return CLASS_E_CLASSNOTAVAILABLE,
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
    PCSTR(c"LSENext 0.0.2".as_ptr() as _)
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

fn menu_command_kinds(root_kind: RootKind, state: Option<SelectionState>) -> Vec<CommandKind> {
    let mut commands = match root_kind {
        RootKind::File | RootKind::Directory => vec![CommandKind::PickSource],
        RootKind::Background => Vec::new(),
    };
    if root_kind == RootKind::File {
        commands.push(CommandKind::Diagnostics);
        return commands;
    }
    if let Some(state) = state {
        commands.push(CommandKind::DropSymbolic);
        if state.sources.iter().all(|source| source.is_dir) {
            commands.push(CommandKind::DropJunction);
        }
        commands.push(CommandKind::ClearSource);
    }
    commands.push(CommandKind::Diagnostics);
    commands
}

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

fn drop_links(items: Option<&IShellItemArray>, kind: LinkKind) -> Result<(), String> {
    let target = shell_item_paths(items)
        .map_err(|err| err.message().to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| "target directory is required".to_string())?;
    let state = load_state()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "Pick a link source before creating a link.".to_string())?;
    for source in &state.sources {
        if let Err(err) = create_link(kind, source, &target) {
            if should_try_elevated(&err) {
                return run_elevated_helper(kind, &target).map_err(|elevated_err| {
                    format!("{}\n\nElevated retry failed: {}", err, elevated_err)
                });
            }
            return Err(err.to_string());
        }
    }
    Ok(())
}

fn should_try_elevated(error: &lsenext_core::links::LinkError) -> bool {
    match error {
        lsenext_core::links::LinkError::CreateFailed { error, .. } => {
            matches!(error.raw_os_error(), Some(5) | Some(1314))
        }
        _ => false,
    }
}

fn run_elevated_helper(kind: LinkKind, target: &Path) -> Result<(), String> {
    let helper = helper_path()
        .ok_or_else(|| "cannot locate LSENext helper next to the shell extension".to_string())?;
    if !helper.is_file() {
        return Err(format!(
            "LSENext helper does not exist: {}",
            helper.display()
        ));
    }
    let command = match kind {
        LinkKind::Symbolic => "drop-symlink",
        LinkKind::Junction => "drop-junction",
    };
    let args = format!("{} \"{}\"", command, target.display());
    let verb: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
    let file: Vec<u16> = helper
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let params: Vec<u16> = args.encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            HWND(std::ptr::null_mut()),
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR(params.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        Err(format!(
            "ShellExecuteW failed with code {}",
            result.0 as isize
        ))
    } else {
        Ok(())
    }
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

fn show_error(message: &str) {
    let title: Vec<u16> = "LSENext".encode_utf16().chain(Some(0)).collect();
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            HWND(std::ptr::null_mut()),
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsenext_core::PickedSource;

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
            vec![
                CommandKind::PickSource,
                CommandKind::DropSymbolic,
                CommandKind::ClearSource,
                CommandKind::Diagnostics,
            ]
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
            vec![
                CommandKind::PickSource,
                CommandKind::DropSymbolic,
                CommandKind::DropJunction,
                CommandKind::ClearSource,
                CommandKind::Diagnostics,
            ]
        );
    }

    #[test]
    fn no_state_only_shows_pick_source() {
        assert_eq!(
            menu_command_kinds(RootKind::Directory, None),
            vec![CommandKind::PickSource, CommandKind::Diagnostics]
        );
    }

    #[test]
    fn file_root_only_shows_pick_source() {
        let state = SelectionState {
            picked_at_unix: 42,
            sources: vec![PickedSource {
                path: PathBuf::from(r"C:\src\folder"),
                is_dir: true,
            }],
        };
        assert_eq!(
            menu_command_kinds(RootKind::File, Some(state)),
            vec![CommandKind::PickSource, CommandKind::Diagnostics]
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
            vec![
                CommandKind::DropSymbolic,
                CommandKind::DropJunction,
                CommandKind::ClearSource,
                CommandKind::Diagnostics,
            ]
        );
    }
}
