//! Runtime translation of bundle placeholders into concrete paths, including
//! Proton prefixes. The bundle carries no platform-specific data; this is
//! runtime policy.

use crate::{
    model::{Platform, Store},
    resolve::{Environment, Install, Probe},
};
use std::path::{Path, PathBuf};

/// Resolve one bundle `dir` template. Returns `None` when a placeholder is
/// unavailable for this install (the candidate is then dropped).
pub(crate) fn resolve(
    template: &str,
    install: &Install,
    environment: &Environment,
    probe: &dyn Probe,
) -> Option<PathBuf> {
    let normalized = template.replace('\\', "/");
    let mut out = String::new();
    let mut rest = normalized.as_str();
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}')? + open;
        let name = &rest[open + 1..close];
        out.push_str(&token(name, install, environment, probe)?);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    if out.contains(['{', '}', '<', '>']) {
        return None;
    }
    if !is_absolute_path(&out) {
        return None;
    }
    let path = PathBuf::from(out);
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(path)
}

/// Host-independent absolute-path test: the resolver must be able to simulate
/// Linux/Proton installs while the tests run on Windows and vice versa.
fn is_absolute_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    text.starts_with('/')
        || text.starts_with("\\\\")
        || (bytes.len() >= 3 && bytes[1] == b':' && matches!(bytes[2], b'/' | b'\\'))
}

fn token(
    name: &str,
    install: &Install,
    environment: &Environment,
    probe: &dyn Probe,
) -> Option<String> {
    if let Some(prefix) = &install.proton_prefix
        && let Some(path) = proton(name, prefix, install, environment, probe)
    {
        return Some(path);
    }
    let folder = |key: &str| {
        environment
            .folders
            .get(key)
            .map(|path| path.to_string_lossy().into_owned())
    };
    match name {
        "INSTALL_DIR" => Some(install.install_dir.to_string_lossy().into_owned()),
        "STORE_USER_ID" => store_user_id(install, environment),
        "STEAM_USERDATA" => environment
            .steam_userdata
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        "APPDATA" | "LOCALAPPDATA" | "LOCALLOW" | "DOCUMENTS" | "PUBLIC" | "PROGRAMDATA"
        | "PROGRAMFILES" | "WINDIR"
            if install.platform == Platform::Windows =>
        {
            folder(name)
        }
        "XDG_DATA_HOME" | "XDG_CONFIG_HOME" if install.platform == Platform::Linux => folder(name),
        "HOME" => folder("HOME"),
        _ => None,
    }
}

fn store_user_id(install: &Install, environment: &Environment) -> Option<String> {
    if let Some(id) = &environment.store_user_id {
        return Some(id.clone());
    }
    if install.store == Store::Steam {
        return environment
            .steam_userdata
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned());
    }
    None
}

fn proton(
    name: &str,
    prefix: &Path,
    install: &Install,
    environment: &Environment,
    probe: &dyn Probe,
) -> Option<String> {
    if name == "INSTALL_DIR" {
        return Some(install.install_dir.to_string_lossy().into_owned());
    }
    let user = profile(prefix, probe);
    let drive_c = prefix.join("drive_c");
    let path = match name {
        "APPDATA" => drive_c.join("users").join(&user).join("AppData/Roaming"),
        "LOCALAPPDATA" => drive_c.join("users").join(&user).join("AppData/Local"),
        "LOCALLOW" => drive_c.join("users").join(&user).join("AppData/LocalLow"),
        "DOCUMENTS" => drive_c.join("users").join(&user).join("Documents"),
        "PUBLIC" => drive_c.join("users/Public"),
        "PROGRAMDATA" => drive_c.join("ProgramData"),
        "PROGRAMFILES" => drive_c.join("Program Files"),
        "WINDIR" => drive_c.join("windows"),
        "HOME" => drive_c.join("users").join(&user),
        "STORE_USER_ID" => return store_user_id(install, environment),
        "STEAM_USERDATA" => {
            return environment
                .steam_userdata
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
        }
        _ => return None,
    };
    Some(path.to_string_lossy().into_owned())
}

/// `steamuser` unless exactly one other `drive_c/users/*` profile exists.
fn profile(prefix: &Path, probe: &dyn Probe) -> String {
    let users = prefix.join("drive_c/users");
    let mut others: Vec<String> = probe
        .child_dirs(&users)
        .into_iter()
        .filter(|name| name != "steamuser")
        .collect();
    others.sort();
    others.dedup();
    match others.as_slice() {
        [only] => only.clone(),
        _ => "steamuser".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::{Environment, Install};
    use std::collections::BTreeMap;

    struct NoProbe;
    impl Probe for NoProbe {
        fn is_dir(&self, _: &Path) -> bool {
            false
        }
        fn install_identity(&self, _: &Path) -> Option<String> {
            None
        }
        fn newest_activity(&self, _: &Path, _: &[String]) -> Option<u64> {
            None
        }
    }

    struct Prefixed {
        prefix: PathBuf,
        profiles: Vec<String>,
    }
    impl Probe for Prefixed {
        fn is_dir(&self, _: &Path) -> bool {
            false
        }
        fn install_identity(&self, _: &Path) -> Option<String> {
            None
        }
        fn newest_activity(&self, _: &Path, _: &[String]) -> Option<u64> {
            None
        }
        fn child_dirs(&self, dir: &Path) -> Vec<String> {
            if dir == self.prefix.join("drive_c/users") {
                self.profiles.clone()
            } else {
                vec![]
            }
        }
    }

    fn windows_install() -> Install {
        Install {
            catalog_id: "steam-1".into(),
            store: Store::Steam,
            platform: Platform::Windows,
            install_dir: PathBuf::from(r"D:\Games\Game"),
            executables: vec![],
            proton_prefix: None,
            identity: None,
        }
    }

    fn environment() -> Environment {
        Environment {
            folders: BTreeMap::from([
                (
                    "APPDATA".into(),
                    PathBuf::from(r"C:\Users\me\AppData\Roaming"),
                ),
                ("HOME".into(), PathBuf::from(r"C:\Users\me")),
            ]),
            steam_userdata: Some(PathBuf::from(r"C:\Steam\userdata\99")),
            current_pick: None,
            store_user_id: None,
        }
    }

    #[test]
    fn resolves_windows_and_steam_placeholders() {
        let probe = NoProbe;
        let env = environment();
        assert_eq!(
            resolve("{INSTALL_DIR}/Saves", &windows_install(), &env, &probe).unwrap(),
            PathBuf::from(r"D:\Games\Game/Saves")
        );
        assert_eq!(
            resolve("{APPDATA}/Void_War", &windows_install(), &env, &probe).unwrap(),
            PathBuf::from(r"C:\Users\me\AppData\Roaming/Void_War")
        );
        assert_eq!(
            resolve(
                "{STEAM_USERDATA}/588650/remote",
                &windows_install(),
                &env,
                &probe
            )
            .unwrap(),
            PathBuf::from(r"C:\Steam\userdata\99/588650/remote")
        );
        assert_eq!(
            resolve(
                "{HOME}/Saved Games/Jagged Alliance 3/{STORE_USER_ID}/*.sav",
                &windows_install(),
                &env,
                &probe
            )
            .unwrap(),
            PathBuf::from(r"C:\Users\me/Saved Games/Jagged Alliance 3/99/*.sav")
        );
    }

    #[test]
    fn drops_unavailable_placeholders() {
        let probe = NoProbe;
        let env = Environment::default();
        assert!(resolve("{APPDATA}/game", &windows_install(), &env, &probe).is_none());
    }

    #[test]
    fn translates_windows_placeholders_into_a_proton_prefix() {
        let prefix = PathBuf::from("/home/me/.steam/steamapps/compatdata/1/pfx");
        let probe = Prefixed {
            prefix: prefix.clone(),
            profiles: vec!["steamuser".into()],
        };
        let install = Install {
            platform: Platform::Linux,
            proton_prefix: Some(prefix.clone()),
            ..windows_install()
        };
        assert_eq!(
            resolve("{APPDATA}/Void_War", &install, &environment(), &probe).unwrap(),
            prefix.join("drive_c/users/steamuser/AppData/Roaming/Void_War")
        );
        // A single non-default profile is used instead of steamuser.
        let probe = Prefixed {
            prefix: prefix.clone(),
            profiles: vec!["steamuser".into(), "alice".into()],
        };
        assert_eq!(
            resolve("{DOCUMENTS}", &install, &environment(), &probe).unwrap(),
            prefix.join("drive_c/users/alice/Documents")
        );
        // Two extra profiles are ambiguous: steamuser wins.
        let probe = Prefixed {
            prefix: prefix.clone(),
            profiles: vec!["steamuser".into(), "alice".into(), "bob".into()],
        };
        assert_eq!(
            resolve("{LOCALAPPDATA}", &install, &environment(), &probe).unwrap(),
            prefix.join("drive_c/users/steamuser/AppData/Local")
        );
    }
}
