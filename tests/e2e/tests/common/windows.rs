//! Windows: a scratch registry key per world, and a job that takes every
//! started process down with the test binary.

/// A throwaway key under `HKCU\Software\SaveScummerTests` for one world's
/// uninstall entries, deleted with the world.
pub mod registry {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegDeleteKeyW, RegDeleteTreeW, RegSetValueExW,
    };

    const PARENT: &str = r"Software\SaveScummerTests";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub struct Scratch {
        pub path: String,
    }

    impl Scratch {
        pub fn new() -> Scratch {
            let path = format!(r"{PARENT}\e2e-{}-{}\Uninstall", std::process::id(), super::super::unique());
            set_value(&path, "", "");
            Scratch { path }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let world = self.path.trim_end_matches(r"\Uninstall");
            delete_tree(world);
            // SAFETY: fails harmlessly while another world still has a key.
            unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(PARENT).as_ptr()) };
        }
    }

    /// Creates `path` (and parents) under HKCU and sets a string value.
    pub fn set_value(path: &str, name: &str, value: &str) {
        let data = wide(value);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: registry calls on this test's own key.
        unsafe {
            let status = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            );
            assert_eq!(status, 0, "create HKCU\\{path}");
            RegSetValueExW(key, wide(name).as_ptr(), 0, REG_SZ, data.as_ptr() as *const u8, (data.len() * 2) as u32);
            RegCloseKey(key);
        }
    }

    pub fn delete_tree(path: &str) {
        assert!(path.starts_with(PARENT), "only scratch keys are deleted");
        // SAFETY: deletes only a scratch key.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(path).as_ptr()) };
        // RegDeleteTreeW leaves the key itself.
        unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(path).as_ptr()) };
    }
}

/// Puts this process in a job that kills everything in it when the last
/// handle closes, which happens when this process exits, however it exits.
pub fn kill_children_on_exit() {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: a new unnamed job whose handle is deliberately kept open for
    // the life of the process; the limit structure is zeroed with its flag set.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return;
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        AssignProcessToJobObject(job, GetCurrentProcess());
    }
}

/// A folder link, the way users make one: a junction, which needs no admin
/// rights.
pub fn link_dir(link: &std::path::Path, target: &std::path::Path) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J", link.to_str().unwrap(), target.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(status.status.success(), "mklink /J failed");
}

/// Removes a folder link made by [`link_dir`], not what it points at.
pub fn unlink_dir(link: &std::path::Path) {
    std::fs::remove_dir(link).unwrap();
}
