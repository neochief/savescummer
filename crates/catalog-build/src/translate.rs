//! Manifest → bundle translation rules (Section 3.1). Pure and deterministic.

use crate::manifest::{Cond, Entry, Whens};
use savescummer_catalog::model::{Detect, IdList, Platform, SaveCandidate, Store, When};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    NoFiles,
    ConfigOnly,
    MicrosoftOnly,
    UnsupportedRoot,
    UnsupportedPlaceholder,
    UnsupportedStore,
    GalaxyStorage,
    BareRoot,
    Empty,
}

impl DropReason {
    pub fn describe(self) -> &'static str {
        match self {
            Self::NoFiles => "no files section",
            Self::ConfigOnly => "config-only entries",
            Self::MicrosoftOnly => "MS Store-only saves",
            Self::UnsupportedRoot => "unsupported <root> path form",
            Self::UnsupportedPlaceholder => "unsupported placeholder",
            Self::UnsupportedStore => "unsupported store condition",
            Self::GalaxyStorage => "GOG Galaxy-managed storage",
            Self::BareRoot => "path resolves to a bare root",
            Self::Empty => "path resolves to nothing",
        }
    }
}

pub fn detect(entry: &Entry) -> Detect {
    let mut steam: Vec<u64> = entry.steam.iter().map(|field| field.id).collect();
    let mut gog: Vec<u64> = entry.gog.iter().map(|field| field.id).collect();
    if let Some(extra) = &entry.id {
        steam.extend(&extra.steam_extra);
        gog.extend(&extra.gog_extra);
    }
    steam.sort_unstable();
    steam.dedup();
    gog.sort_unstable();
    gog.dedup();
    Detect {
        steam: (!steam.is_empty()).then_some(IdList(steam)),
        gog: (!gog.is_empty()).then_some(IdList(gog)),
    }
}

/// `launch` → `executables`, bucketed per OS. `bit` and `store` are ignored.
pub fn executables(entry: &Entry) -> (BTreeMap<Platform, Vec<String>>, Vec<String>) {
    let mut buckets: BTreeMap<Platform, BTreeSet<String>> = BTreeMap::new();
    let mut warnings = Vec::new();
    for (path, items) in entry.launch.iter().flatten() {
        let Some(relative) = path.strip_prefix("<base>/") else {
            warnings.push(format!(
                "launch path {path:?} does not start with <base>/; ignored"
            ));
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        for item in items {
            for platform in launch_platforms(Whens::slice_or_default(&item.when)) {
                buckets
                    .entry(platform)
                    .or_default()
                    .insert(relative.to_string());
            }
        }
    }
    let buckets = buckets
        .into_iter()
        .map(|(platform, values)| (platform, values.into_iter().collect()))
        .collect();
    (buckets, warnings)
}

fn launch_platforms(conditions: &[Cond]) -> Vec<Platform> {
    if conditions.is_empty() {
        return vec![Platform::Windows, Platform::Macos, Platform::Linux];
    }
    let mut platforms = BTreeSet::new();
    for condition in conditions {
        match condition.os.as_deref() {
            None => {
                platforms.extend([Platform::Windows, Platform::Macos, Platform::Linux]);
            }
            Some(os) => {
                if let Some(platform) = parse_os(os) {
                    platforms.insert(platform);
                }
            }
        }
    }
    platforms.into_iter().collect()
}

/// `files` → save candidates, with dropped reasons for the report.
pub fn save(entry: &Entry) -> (Vec<SaveCandidate>, Vec<DropReason>) {
    let mut candidates = Vec::new();
    let mut drops = Vec::new();
    let Some(files) = &entry.files else {
        return (candidates, vec![DropReason::NoFiles]);
    };
    for (path, file) in files {
        if !file.tags.is_empty() && !file.tags.iter().any(|tag| tag == "save") {
            drops.push(DropReason::ConfigOnly);
            continue;
        }
        let conditions = Whens::slice_or_default(&file.when);
        let conditions: Vec<Cond> = if conditions.is_empty() {
            vec![Cond::default()]
        } else {
            conditions.to_vec()
        };
        if conditions
            .iter()
            .all(|condition| condition.store.as_deref() == Some("microsoft"))
        {
            drops.push(DropReason::MicrosoftOnly);
            continue;
        }
        for condition in conditions {
            let store = match condition.store.as_deref() {
                None => None,
                Some("microsoft") => {
                    drops.push(DropReason::MicrosoftOnly);
                    continue;
                }
                Some("steam") => Some(Store::Steam),
                Some("gog") => Some(Store::Gog),
                Some("epic") => Some(Store::Epic),
                Some("standalone") => Some(Store::Standalone),
                Some(_) => {
                    drops.push(DropReason::UnsupportedStore);
                    continue;
                }
            };
            let os = match condition.os.as_deref() {
                None => None,
                Some(os) => match parse_os(os) {
                    Some(platform) => Some(platform),
                    None => {
                        drops.push(DropReason::UnsupportedPlaceholder);
                        continue;
                    }
                },
            };
            match normalize_path(path) {
                Ok(dir) => candidates.push(SaveCandidate {
                    when: (os.is_some() || store.is_some()).then_some(When { os, store }),
                    dir,
                }),
                Err(reason) => drops.push(reason),
            }
        }
    }
    (deduplicate(candidates), drops)
}

fn parse_os(os: &str) -> Option<Platform> {
    match os {
        "windows" => Some(Platform::Windows),
        "mac" | "macos" => Some(Platform::Macos),
        "linux" => Some(Platform::Linux),
        _ => None,
    }
}

const PLACEHOLDER_ROOTS: &[(&str, &str)] = &[
    ("<home>", "{HOME}"),
    ("<winAppData>", "{APPDATA}"),
    ("<winLocalAppData>", "{LOCALAPPDATA}"),
    ("<winLocalAppDataLow>", "{LOCALLOW}"),
    ("<winDocuments>", "{DOCUMENTS}"),
    ("<winProgramData>", "{PROGRAMDATA}"),
    ("<winPublic>", "{PUBLIC}"),
    ("<winDir>", "{WINDIR}"),
    ("<xdgData>", "{XDG_DATA_HOME}"),
    ("<xdgConfig>", "{XDG_CONFIG_HOME}"),
    ("<storeUserId>", "{STORE_USER_ID}"),
];

/// Translate one manifest path into a bundle `dir` template.
pub fn normalize_path(path: &str) -> Result<String, DropReason> {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let replaced = if let Some(rest) = trimmed.strip_prefix("<base>") {
        if rest.is_empty() {
            return Err(DropReason::BareRoot);
        }
        format!("{{INSTALL_DIR}}{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("<root>") {
        if let Some(rest) = rest.strip_prefix("/steamapps/common/") {
            let rest = strip_install_dir_segment(rest);
            if rest.is_empty() {
                return Err(DropReason::BareRoot);
            }
            format!("{{INSTALL_DIR}}{rest}")
        } else if let Some(rest) = rest.strip_prefix("/userdata/") {
            let rest = rest.strip_prefix("<storeUserId>/").unwrap_or(rest);
            if rest.is_empty() {
                return Err(DropReason::BareRoot);
            }
            format!("{{STEAM_USERDATA}}/{rest}")
        } else {
            return Err(DropReason::UnsupportedRoot);
        }
    } else {
        if trimmed.contains("<base>") {
            return Err(DropReason::UnsupportedRoot);
        }
        let mut replaced = trimmed.to_string();
        for (from, to) in PLACEHOLDER_ROOTS {
            replaced = replaced.replace(from, to);
        }
        replaced
    };
    let replaced = replaced.replace('\\', "/");
    if replaced.contains("<base>") || replaced.contains("<root>") {
        return Err(DropReason::UnsupportedRoot);
    }
    if first_unknown_placeholder(&replaced).is_some() {
        return Err(DropReason::UnsupportedPlaceholder);
    }
    let collapsed = collapse_glob(&replaced);
    let dir = to_directory(&collapsed);
    let dir = dir.trim_end_matches('/').to_string();
    if dir.is_empty() || dir == "/" {
        return Err(DropReason::Empty);
    }
    if is_bare_root(&dir) {
        return Err(DropReason::BareRoot);
    }
    if dir.to_ascii_lowercase().contains("gog.com/galaxy") {
        return Err(DropReason::GalaxyStorage);
    }
    Ok(dir)
}

/// `<root>/steamapps/common/<installDir>/rest` → `/rest`.
fn strip_install_dir_segment(rest: &str) -> String {
    strip_segment(rest)
}

/// Drop the first path segment, keeping the leading slash.
fn strip_segment(rest: &str) -> String {
    match rest.find('/') {
        Some(index) => rest[index..].to_string(),
        None => String::new(),
    }
}

fn first_unknown_placeholder(path: &str) -> Option<String> {
    let start = path.find('<')?;
    let end = path[start..].find('>')? + start;
    Some(path[start..=end].to_string())
}

/// Cut at the first glob metacharacter, then drop the trailing partial segment.
fn collapse_glob(path: &str) -> String {
    let Some(index) = path.find(['*', '?', '[']) else {
        return path.to_string();
    };
    let before = &path[..index];
    match before.rfind('/') {
        Some(slash) => before[..slash].to_string(),
        None => String::new(),
    }
}

/// File-looking leaf paths resolve to their parent directory.
fn to_directory(path: &str) -> String {
    let last = path.rsplit('/').next().unwrap_or(path);
    if last.contains('.') && last != "." && last != ".." {
        match path.rfind('/') {
            Some(slash) => path[..slash].to_string(),
            None => String::new(),
        }
    } else {
        path.to_string()
    }
}

fn is_bare_root(dir: &str) -> bool {
    !dir.contains('/')
        && matches!(
            dir,
            "{INSTALL_DIR}"
                | "{HOME}"
                | "{APPDATA}"
                | "{LOCALAPPDATA}"
                | "{LOCALLOW}"
                | "{DOCUMENTS}"
                | "{PUBLIC}"
                | "{PROGRAMDATA}"
                | "{PROGRAMFILES}"
                | "{WINDIR}"
                | "{XDG_DATA_HOME}"
                | "{XDG_CONFIG_HOME}"
                | "{STEAM_USERDATA}"
        )
}

/// Merge candidates that normalize to the same directory when compatible. The
/// `<base>`/`<root>` spellings of one directory collapse to one candidate
/// (Section 3.1.3); conflicting OS/store conditions stay separate.
fn deduplicate(candidates: Vec<SaveCandidate>) -> Vec<SaveCandidate> {
    let mut merged: Vec<SaveCandidate> = Vec::new();
    for candidate in candidates {
        let existing = merged
            .iter_mut()
            .find(|other| other.dir == candidate.dir && compatible(other.when, candidate.when));
        match existing {
            Some(other) => other.when = combine(other.when, candidate.when),
            None => merged.push(candidate),
        }
    }
    merged.sort_by(|a, b| {
        a.dir
            .cmp(&b.dir)
            .then_with(|| key(a.when).cmp(&key(b.when)))
    });
    merged
}

fn compatible(a: Option<When>, b: Option<When>) -> bool {
    fn ok(left: Option<Platform>, right: Option<Platform>) -> bool {
        match (left, right) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }
    fn store_ok(left: Option<Store>, right: Option<Store>) -> bool {
        match (left, right) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }
    let a = a.unwrap_or(When {
        os: None,
        store: None,
    });
    let b = b.unwrap_or(When {
        os: None,
        store: None,
    });
    ok(a.os, b.os) && store_ok(a.store, b.store)
}

fn combine(a: Option<When>, b: Option<When>) -> Option<When> {
    let a = a.unwrap_or(When {
        os: None,
        store: None,
    });
    let b = b.unwrap_or(When {
        os: None,
        store: None,
    });
    let when = When {
        os: a.os.or(b.os),
        store: a.store.or(b.store),
    };
    (!when.is_any()).then_some(when)
}

fn key(when: Option<When>) -> (u8, u8, u8, u8) {
    let when = when.unwrap_or(When {
        os: None,
        store: None,
    });
    (
        when.os.map(platform_rank).unwrap_or(3),
        when.store.map(store_rank).unwrap_or(4),
        0,
        0,
    )
}

fn platform_rank(platform: Platform) -> u8 {
    match platform {
        Platform::Windows => 0,
        Platform::Macos => 1,
        Platform::Linux => 2,
    }
}

fn store_rank(store: Store) -> u8 {
    match store {
        Store::Steam => 0,
        Store::Gog => 1,
        Store::Epic => 2,
        Store::Standalone => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;

    fn entry(yaml: &str) -> Entry {
        manifest::parse(&format!("Game:\n{yaml}"))
            .unwrap()
            .remove("Game")
            .unwrap()
    }

    #[test]
    fn highfleet_translates_to_three_windows_candidates() {
        let entry = entry(
            r#"  files:
    "<base>/Config.ini":
      tags: [config]
      when:
        - os: windows
    "<base>/Saves":
      tags: [save]
      when:
        - os: windows
    "<base>/SavesSkirmish":
      tags: [save]
      when:
        - os: windows
    "<base>/Ships":
      tags: [save]
      when:
        - os: windows
    "<root>/steamapps/common/HighFleet/Config.ini":
      tags: [config]
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/Saves":
      tags: [save]
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/SavesSkirmish":
      tags: [save]
      when:
        - store: steam
    "<root>/steamapps/common/HighFleet/Ships":
      tags: [save]
      when:
        - store: steam
  launch:
    "<base>/Highfleet.exe":
      - when:
          - os: windows
            store: steam
"#,
        );
        let (save, drops) = save(&entry);
        assert!(
            drops.iter().all(|reason| *reason == DropReason::ConfigOnly),
            "{drops:?}"
        );
        assert_eq!(
            save.iter().map(|c| c.dir.as_str()).collect::<Vec<_>>(),
            vec![
                "{INSTALL_DIR}/Saves",
                "{INSTALL_DIR}/SavesSkirmish",
                "{INSTALL_DIR}/Ships"
            ]
        );
        assert!(
            save.iter()
                .all(|c| c.when.unwrap().os == Some(Platform::Windows))
        );
        let (executables, _) = executables(&entry);
        assert_eq!(executables[&Platform::Windows], vec!["Highfleet.exe"]);
    }

    #[test]
    fn globs_collapse_files_resolve_and_userdata_is_special() {
        assert_eq!(
            normalize_path("<home>/Saved Games/Jagged Alliance 3/<storeUserId>/*.sav").unwrap(),
            "{HOME}/Saved Games/Jagged Alliance 3/{STORE_USER_ID}"
        );
        assert_eq!(
            normalize_path("<winAppData>/game/settings.ini").unwrap(),
            "{APPDATA}/game"
        );
        assert_eq!(
            normalize_path("<root>/userdata/588650/remote").unwrap(),
            "{STEAM_USERDATA}/588650/remote"
        );
        assert_eq!(
            normalize_path("<root>/userdata/<storeUserId>/632360/remote/UserProfiles").unwrap(),
            "{STEAM_USERDATA}/632360/remote/UserProfiles"
        );
        assert_eq!(
            normalize_path("<root>/steamapps/common/Game/Saves").unwrap(),
            "{INSTALL_DIR}/Saves"
        );
        assert_eq!(
            normalize_path("<root>/savegames/<storeUserId>/3353"),
            Err(DropReason::UnsupportedRoot)
        );
        assert_eq!(
            normalize_path("<base>/x/<base>/y"),
            Err(DropReason::UnsupportedRoot)
        );
        assert_eq!(normalize_path("<winDocuments>"), Err(DropReason::BareRoot));
        assert_eq!(
            normalize_path(
                "<winLocalAppData>/GOG.com/Galaxy/Applications/50593543263669699/Storage/Shared/Files/C*/SGS*"
            ),
            Err(DropReason::GalaxyStorage)
        );
    }

    #[test]
    fn keeps_untagged_entries_and_ignores_trailing_slashes() {
        let entry = entry(
            r#"  files:
    "<winAppData>/untagged/":
      when:
        - os: windows
    "<winAppData>/config":
      tags: [config]
      when:
        - os: windows
"#,
        );
        let (save, drops) = save(&entry);
        assert_eq!(save.len(), 1);
        assert_eq!(save[0].dir, "{APPDATA}/untagged");
        assert_eq!(drops, vec![DropReason::ConfigOnly]);
    }

    #[test]
    fn drops_ms_store_only_and_unsupported_stores() {
        let entry = entry(
            r#"  files:
    "<winAppData>/ms":
      tags: [save]
      when:
        - store: microsoft
    "<winAppData>/origin":
      tags: [save]
      when:
        - store: origin
"#,
        );
        let (save, drops) = save(&entry);
        assert!(save.is_empty());
        assert!(drops.contains(&DropReason::MicrosoftOnly));
        assert!(drops.contains(&DropReason::UnsupportedStore));
    }
}
