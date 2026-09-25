//! The macOS login item: an agent registered with `SMAppService.agent`
//! (macOS 13+). Its plist ships inside the bundle
//! (`Contents/Library/LaunchAgents`) and runs the host with `--minimized`,
//! so macOS lists it as SaveScummer in Login Items, the user can turn it
//! off there, and nothing outside the bundle can go stale.
//!
//! The registration belongs to the bundle the host runs from: `off` can't
//! remove another copy's.

use std::path::Path;

use objc2_foundation::NSString;
use objc2_service_management::{SMAppService, SMAppServiceStatus};

/// The agent's plist in `Contents/Library/LaunchAgents`.
const PLIST: &str = "com.savescummer.SaveScummer.host.plist";

pub fn set(on: bool, host_exe: &Path, data_dir: Option<&Path>) -> Result<(), String> {
    if !in_bundle(host_exe) {
        return if on { Err("only the app bundle can launch at login".into()) } else { Ok(()) };
    }
    // The plist is fixed at build time: it can't carry `--data-dir`.
    if on && data_dir.is_some_and(|d| d != crate::data_dir()) {
        return Err("launch at login always uses the default data folder on macOS; drop --data-dir".into());
    }
    let service = service();
    // SAFETY: plain ServiceManagement calls on our own bundle's agent.
    let result = unsafe {
        match (on, service.status()) {
            (true, _) => service.registerAndReturnError(),
            (false, SMAppServiceStatus::Enabled | SMAppServiceStatus::RequiresApproval) => {
                service.unregisterAndReturnError()
            }
            // Not registered: nothing of ours to remove.
            (false, _) => Ok(()),
        }
    };
    result.map_err(|e| e.localizedDescription().to_string())
}

/// On only while macOS will run it: turned off in System Settings, it
/// "requires approval" and counts as off.
pub fn is_enabled(host_exe: &Path) -> bool {
    // SAFETY: a status query.
    in_bundle(host_exe) && unsafe { service().status() } == SMAppServiceStatus::Enabled
}

/// Registered, but the user turned it off in System Settings: only they can
/// turn it on again there.
pub fn needs_approval(host_exe: &Path) -> bool {
    // SAFETY: a status query.
    in_bundle(host_exe) && unsafe { service().status() } == SMAppServiceStatus::RequiresApproval
}

fn service() -> objc2::rc::Retained<SMAppService> {
    // SAFETY: the plist name is a plain string; the service refers to this
    // process's main bundle.
    unsafe { SMAppService::agentServiceWithPlistName(&NSString::from_str(PLIST)) }
}

/// `…/SaveScummer.app/Contents/MacOS/<host>`: a dev build run from
/// `target/` has no bundle and no agent to register.
fn in_bundle(host_exe: &Path) -> bool {
    host_exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::extension)
        .is_some_and(|e| e == "app")
}
