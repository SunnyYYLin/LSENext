#![allow(non_snake_case)]

use lsenext_core::{clear_state, create_link, load_state, save_sources, LinkKind};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows::core::{implement, Interface, GUID, HRESULT, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    BOOL, CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_NOTIMPL, E_OUTOFMEMORY,
    E_POINTER, HINSTANCE, HWND, S_FALSE,
};
use windows::Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows::Win32::UI::Shell::{
    ECF_DEFAULT, ECS_DISABLED, ECS_ENABLED, ECS_HIDDEN, IEnumExplorerCommand,
    IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl, IShellItemArray,
    ShellExecuteW, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK, SW_SHOWNORMAL};

pub const CLSID_LSENEXT_ROOT: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e000);
pub const CLSID_LSENEXT_PICK_SOURCE: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e001);
pub const CLSID_LSENEXT_DROP_SYMLINK: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e002);
pub const CLSID_LSENEXT_DROP_JUNCTION: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e003);
pub const CLSID_LSENEXT_CLEAR_SOURCE: GUID =
    GUID::from_u128(0x32ad61d5_1919_4582_95dc_d9eb0bb6e004);

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandKind {
    PickSource,
    DropSymbolic,
    DropJunction,
    ClearSource,
}

#[implement(IExplorerCommand)]
struct RootCommand;

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
        Ok(PWSTR::null())
    }

    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        alloc_pwstr("LSENext")
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        Ok(CLSID_LSENEXT_ROOT)
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
        enumerate_commands()
    }
}

impl IExplorerCommand_Impl for MenuCommand_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        alloc_pwstr(match self.kind {
            CommandKind::PickSource => "Pick Link Source",
            CommandKind::DropSymbolic => "Drop Symbolic Link",
            CommandKind::DropJunction => "Drop Directory Junction",
            CommandKind::ClearSource => "Clear Link Source",
        })
    }

    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        Ok(PWSTR::null())
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
        })
    }

    fn GetState(
        &self,
        _items: Option<&IShellItemArray>,
        _ok_to_be_slow: BOOL,
    ) -> windows::core::Result<u32> {
        let state = match self.kind {
            CommandKind::PickSource => ECS_ENABLED.0,
            CommandKind::DropSymbolic | CommandKind::ClearSource => {
                if load_state().ok().flatten().is_some() {
                    ECS_ENABLED.0
                } else {
                    ECS_DISABLED.0
                }
            }
            CommandKind::DropJunction => {
                if junction_available() {
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
                .and_then(|paths| save_sources(&paths).map(|_| ()).map_err(|err| err.to_string())),
            CommandKind::DropSymbolic => drop_links(items, LinkKind::Symbolic),
            CommandKind::DropJunction => drop_links(items, LinkKind::Junction),
            CommandKind::ClearSource => clear_state().map(|_| ()).map_err(|err| err.to_string()),
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
struct ClassFactory;

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

        let command: IExplorerCommand = RootCommand.into();
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

    let is_root = unsafe { *rclsid == CLSID_LSENEXT_ROOT };
    if !is_root {
        return CLASS_E_CLASSNOTAVAILABLE;
    }

    let factory: IClassFactory = ClassFactory.into();
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
    PCSTR(c"LSENext 0.0.1".as_ptr() as _)
}

fn enumerate_commands() -> windows::core::Result<IEnumExplorerCommand> {
    let mut commands = vec![MenuCommand { kind: CommandKind::PickSource }.into()];

    if has_source_state() {
        commands.push(MenuCommand { kind: CommandKind::DropSymbolic }.into());
        if junction_available() {
            commands.push(MenuCommand { kind: CommandKind::DropJunction }.into());
        }
        commands.push(MenuCommand { kind: CommandKind::ClearSource }.into());
    }

    Ok(CommandEnum {
        commands,
        index: std::cell::Cell::new(0),
    }
    .into())
}

fn has_source_state() -> bool {
    load_state().ok().flatten().is_some()
}

fn junction_available() -> bool {
    load_state()
        .ok()
        .flatten()
        .map(|state| state.sources.iter().all(|source| source.is_dir))
        .unwrap_or(false)
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
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let helper = exe
        .parent()
        .ok_or_else(|| "cannot locate LSENext helper next to the shell extension".to_string())?
        .join("lsenext-helper.exe");
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
        Err(format!("ShellExecuteW failed with code {}", result.0 as isize))
    } else {
        Ok(())
    }
}

fn shell_item_paths(items: Option<&IShellItemArray>) -> windows::core::Result<Vec<PathBuf>> {
    let items = items.ok_or_else(|| windows::core::Error::new(E_FAIL, "Explorer did not provide a selected item."))?;
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
