//! Thin Explorer ABI: paths become host-issued checkpoint IDs; file operations
//! and policy remain in the host. Fail closed on timeout or malformed replies.
use savescummer_core::*;
use savescummer_ipc::*;
use std::{path::PathBuf, time::Duration};

pub struct Menu {
    endpoint: PathBuf,
    targets: Vec<ExplorerTarget>,
    request_id: Id,
}
fn query(endpoint: &std::path::Path, command: Command, request_id: Id) -> std::io::Result<Reply> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_millis(750),
            savescummer_ipc::request(
                endpoint,
                &Request {
                    version: VERSION,
                    request_id,
                    command,
                },
            ),
        )
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "host response timeout"))?
        .map(|response| response.result)
    })
}
/// # Safety
/// `path` must point to `length` readable UTF-16 units and `output` to writable
/// pointer storage. Caller releases the returned allocation with sc_menu_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sc_menu(path: *const u16, length: usize, output: *mut *mut Menu) -> u32 {
    if path.is_null() || output.is_null() || length == 0 || length > 32768 {
        return 0;
    }
    unsafe {
        *output = std::ptr::null_mut();
    }
    std::panic::catch_unwind(|| {
        let path = String::from_utf16(unsafe { std::slice::from_raw_parts(path, length) }).ok()?;
        let folders = savescummer_platform::known_folders();
        // Baked into the dedicated development DLL; Explorer does not inherit
        // the invoking terminal's environment. Ordinary builds leave this empty.
        let directory = match option_env!("SAVESCUMMER_EXPLORER_DEV_DATA_DIR") {
            Some(path) if !path.is_empty() => PathBuf::from(path),
            _ => folders.get("LOCALAPPDATA")?.join("SaveScummer"),
        };
        let endpoint = endpoint(&directory).ok()?;
        let Reply::ExplorerTargets { targets } = query(
            &endpoint,
            Command::ExplorerTargets { path: path.into() },
            new_id(),
        )
        .ok()?
        else {
            return None;
        };
        // Overlapping matches are never guessed by a shell integration.
        if targets.len() != 1 {
            return None;
        }
        let kind = match targets[0].action {
            Action::Save => 1,
            Action::Load { target: Some(_) } => 2,
            _ => return None,
        };
        let menu = Box::new(Menu {
            endpoint,
            targets,
            request_id: new_id(),
        });
        unsafe {
            *output = Box::into_raw(menu);
        }
        Some(kind)
    })
    .ok()
    .flatten()
    .unwrap_or(0)
}
/// # Safety
/// `menu` must be a live pointer returned by sc_menu, not used concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sc_invoke(menu: *const Menu) -> bool {
    if menu.is_null() {
        return false;
    }
    std::panic::catch_unwind(|| {
        let menu = unsafe { &*menu };
        let target = &menu.targets[0];
        matches!(
            query(
                &menu.endpoint,
                Command::Execute {
                    game_id: target.game_id.clone(),
                    action: target.action.clone()
                },
                menu.request_id.clone()
            ),
            Ok(Reply::Accepted { .. })
        )
    })
    .unwrap_or(false)
}
/// # Safety
/// The pointer must be null or a pointer returned by sc_menu, freed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sc_menu_free(menu: *mut Menu) {
    if !menu.is_null() {
        drop(unsafe { Box::from_raw(menu) });
    }
}
