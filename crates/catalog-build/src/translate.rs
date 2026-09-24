//! Manifest entry → catalog entry (PLAN-CATALOG.md 3.1).
//!
//! Pure functions over one [`ManifestGame`]: no report, no addendum. What was
//! dropped and why comes back in the [`Translation`] so the caller decides
//! what to warn about.

use std::collections::BTreeSet;
use std::fmt;

use savescummer_catalog::broad::is_broad_template;
use savescummer_catalog::glob::has_wildcard;
use savescummer_catalog::model::validate_template;
use savescummer_catalog::{Detect, Executables, PathRule, Platform, Store, When};

use crate::manifest::{Condition, FileEntry, ManifestGame};

/// The entry fields the builder produces, before identity and `info`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub detect: Detect,
    pub install_dirs: Vec<String>,
    pub executables: Executables,
    pub save: Vec<PathRule>,
    pub exclude: Vec<PathRule>,
}

/// Why a manifest path produced no target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DropReason {
    /// Every condition names a store or OS we never discover (rule 3).
    UnsupportedStore,
    /// A Steam userdata path we can't map to `{STEAM_USERDATA}/<appId>`.
    Userdata,
    /// Unknown `<root>/...`, a placeholder that isn't leading or has no
    /// bundle equivalent, launcher-managed storage, or a path the bundle
    /// can't spell (rule 7).
    UnsupportedPath,
    /// Takes a broad folder or a wildcard directly inside one (rule 7).
    Broad,
}

impl DropReason {
    /// Whether rule 7 asks for a build warning. Unsupported stores are
    /// dropped silently (rule 3): they are out of scope, not lost.
    pub fn warns(self) -> bool {
        !matches!(self, DropReason::UnsupportedStore)
    }
}

impl fmt::Display for DropReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DropReason::UnsupportedStore => "only unsupported stores or OSes",
            DropReason::Userdata => "Steam userdata path without an app id",
            DropReason::UnsupportedPath => "unsupported path form",
            DropReason::Broad => "broad folder",
        })
    }
}

/// A save candidate that didn't become a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// The manifest's own spelling of the path.
    pub manifest_path: String,
    pub reason: DropReason,
    pub detail: String,
}

/// The reason category of a game with no usable save target (Section 3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    NoFiles,
    ConfigOnly,
    UserdataOnly,
    MsStoreOnly,
    UnsupportedPathForm,
    BroadFolderOnly,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::NoFiles => "no files section",
            Category::ConfigOnly => "config-only",
            Category::UserdataOnly => "userdata-only",
            Category::MsStoreOnly => "MS-Store-only (or other undiscovered stores)",
            Category::UnsupportedPathForm => "unsupported path form",
            Category::BroadFolderOnly => "broad folder only",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Translation {
    pub entry: Entry,
    /// Save candidates that were dropped, in manifest order.
    pub dropped: Vec<Dropped>,
    /// When `entry.save` is empty: why, as one or more categories.
    pub failure: Vec<Category>,
    /// Save targets that came from entries tagged both config and save and
    /// look like whole folders (a literal path whose last name has no
    /// extension). Listed for review (Section 3.5).
    pub config_save_folders: BTreeSet<String>,
    /// `launch` keys that couldn't become executables, with why.
    pub dropped_launch: Vec<(String, String)>,
}

/// Translates one manifest entry. `name` is the manifest key (used only as
/// the `<game>` fallback when the entry has no `installDir`).
pub fn translate(name: &str, game: &ManifestGame) -> Translation {
    let mut out = Translation::default();

    // Identity and detection.
    let mut steam: Vec<u64> = game.steam.as_ref().and_then(|s| s.id).into_iter().collect();
    let mut gog: Vec<u64> = game.gog.as_ref().and_then(|g| g.id).into_iter().collect();
    if let Some(extra) = &game.id {
        steam.extend(&extra.steam_extra);
        gog.extend(&extra.gog_extra);
    }
    dedupe(&mut steam);
    dedupe(&mut gog);
    out.entry.detect = Detect { steam, gog, uninstall: Vec::new() };
    out.entry.install_dirs = game.install_dir.keys().cloned().collect();

    translate_launch(game, &mut out);

    let ctx = Context {
        steam_id: out.entry.detect.steam.first().copied(),
        gog_id: out.entry.detect.gog.first().copied(),
        game_dirs: if out.entry.install_dirs.is_empty() {
            vec![name.to_string()]
        } else {
            out.entry.install_dirs.clone()
        },
    };

    let Some(files) = &game.files else {
        out.failure = vec![Category::NoFiles];
        return out;
    };
    if files.is_empty() {
        out.failure = vec![Category::NoFiles];
        return out;
    }

    // Rule 4: `<base>/X` and `<root>/steamapps/common/<dir>/X` are one path.
    // Remember every `<base>` spelling so the `<root>` one can be skipped.
    let base_paths: BTreeSet<String> = files
        .keys()
        .filter(|k| clean(k).starts_with("<base>/"))
        .flat_map(|k| map_path(k, When::default(), &ctx).ok().map(|m| m.paths).unwrap_or_default())
        .map(|p| p.to_lowercase())
        .collect();

    let mut saves: Vec<PathRule> = Vec::new();
    let mut configs: Vec<PathRule> = Vec::new();
    let mut save_candidates = 0usize;
    for (raw, entry) in files {
        let kind = Kind::of(entry);
        if kind == Kind::Ignored {
            continue;
        }
        if kind == Kind::Save {
            save_candidates += 1;
        }
        let conditions = conditions(&entry.when);
        if conditions.is_empty() {
            if kind == Kind::Save {
                out.dropped.push(Dropped {
                    manifest_path: raw.clone(),
                    reason: DropReason::UnsupportedStore,
                    detail: "only conditions naming stores or OSes that are never discovered".into(),
                });
            }
            continue;
        }
        let mut last_error: Option<(DropReason, String)> = None;
        let mut produced = false;
        for when in conditions {
            let mapped = match map_path(raw, when, &ctx) {
                Ok(mapped) => mapped,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };
            if mapped.via_root_common && mapped.paths.iter().all(|p| base_paths.contains(&p.to_lowercase())) {
                // The same file is listed under `<base>`; that spelling wins.
                produced = true;
                continue;
            }
            for path in mapped.paths {
                if kind == Kind::Save && is_broad_template(&path) {
                    last_error = Some((DropReason::Broad, format!("{path} takes a broad folder")));
                    continue;
                }
                let rule = PathRule { when, path };
                if kind == Kind::Save {
                    if entry.tags.iter().any(|t| t == "config") && looks_like_folder(&rule.path) {
                        out.config_save_folders.insert(rule.path.clone());
                    }
                    saves.push(rule);
                } else {
                    configs.push(rule);
                }
                produced = true;
            }
        }
        if !produced && kind == Kind::Save {
            let (reason, detail) = last_error.unwrap_or((DropReason::UnsupportedPath, "no usable spelling".into()));
            out.dropped.push(Dropped { manifest_path: raw.clone(), reason, detail });
        } else if kind == Kind::Save
            && let Some((DropReason::Broad, detail)) = last_error
        {
            // Some conditions produced a target but another spelling was
            // broad: still a warning, so a lost OS never goes unnoticed.
            out.dropped.push(Dropped { manifest_path: raw.clone(), reason: DropReason::Broad, detail });
        }
    }

    out.entry.save = normalize(saves);

    // Rule 2: config-only entries inside a save target become excludes.
    let excludes: Vec<PathRule> =
        configs.into_iter().filter(|config| out.entry.save.iter().any(|target| inside(config, target))).collect();
    out.entry.exclude = normalize(excludes);

    if out.entry.save.is_empty() {
        out.failure = if save_candidates == 0 {
            vec![Category::ConfigOnly]
        } else {
            let categories: BTreeSet<Category> = out
                .dropped
                .iter()
                .map(|d| match d.reason {
                    DropReason::UnsupportedStore => Category::MsStoreOnly,
                    DropReason::Userdata => Category::UserdataOnly,
                    DropReason::UnsupportedPath => Category::UnsupportedPathForm,
                    DropReason::Broad => Category::BroadFolderOnly,
                })
                .collect();
            categories.into_iter().collect()
        };
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Tagged `save`, or untagged (rule 1).
    Save,
    /// Tagged only `config` (rule 2).
    Config,
    /// Tagged only with something else: neither.
    Ignored,
}

impl Kind {
    fn of(entry: &FileEntry) -> Kind {
        if entry.tags.is_empty() || entry.tags.iter().any(|t| t == "save") {
            Kind::Save
        } else if entry.tags.iter().any(|t| t == "config") {
            Kind::Config
        } else {
            Kind::Ignored
        }
    }
}

/// Maps a manifest `when` list to bundle conditions (rule 8). An empty list
/// means "always". Conditions naming a store or OS we don't support can
/// never apply and are dropped; `bit` isn't read at all.
fn conditions(when: &[Condition]) -> Vec<When> {
    if when.is_empty() {
        return vec![When::default()];
    }
    let mut out = Vec::new();
    for condition in when {
        let os = match condition.os.as_deref() {
            None => None,
            Some("windows") => Some(Platform::Windows),
            Some("mac") => Some(Platform::Macos),
            Some("linux") => Some(Platform::Linux),
            Some(_) => continue,
        };
        let store = match condition.store.as_deref() {
            None => None,
            Some(store) => match Store::parse(store) {
                // The manifest never says "standalone"; if it did, it would
                // mean something else than our own store bucket.
                Some(Store::Standalone) | None => continue,
                known => known,
            },
        };
        let when = When { os, store };
        if !out.contains(&when) {
            out.push(when);
        }
    }
    out
}

struct Context {
    steam_id: Option<u64>,
    gog_id: Option<u64>,
    /// What `<game>` and `<root>/steamapps/common/<name>` may be.
    game_dirs: Vec<String>,
}

struct Mapped {
    paths: Vec<String>,
    /// Spelled as `<root>/steamapps/common/<dir>/...` (rule 4).
    via_root_common: bool,
}

/// Leading Ludusavi roots and their bundle placeholders.
const ROOTS: &[(&str, &str)] = &[
    ("<base>", "{INSTALL_DIR}"),
    ("<home>", "{HOME}"),
    ("<winAppData>", "{APPDATA}"),
    ("<winLocalAppData>", "{LOCALAPPDATA}"),
    ("<winLocalAppDataLow>", "{LOCALLOW}"),
    ("<winDocuments>", "{DOCUMENTS}"),
    ("<winPublic>", "{PUBLIC}"),
    ("<winProgramData>", "{PROGRAMDATA}"),
    ("<winDir>", "{WINDIR}"),
    ("<xdgData>", "{XDG_DATA_HOME}"),
    ("<xdgConfig>", "{XDG_CONFIG_HOME}"),
];

/// Maps one manifest path under one condition to bundle templates.
fn map_path(raw: &str, when: When, ctx: &Context) -> Result<Mapped, (DropReason, String)> {
    let path = clean(raw);
    let unsupported = |why: String| (DropReason::UnsupportedPath, why);
    if path.to_lowercase().contains("gog.com/galaxy/applications") {
        return Err(unsupported("GOG Galaxy-managed storage".into()));
    }
    let (first, rest) = match path.split_once('/') {
        Some((first, rest)) => (first, rest),
        None => (path.as_str(), ""),
    };

    let mut via_root_common = false;
    let (root, rest): (String, String) = if let Some((_, placeholder)) = ROOTS.iter().find(|(r, _)| *r == first) {
        (placeholder.to_string(), rest.to_string())
    } else if first == "<root>" {
        let segments: Vec<&str> = rest.split('/').collect();
        match segments.as_slice() {
            ["steamapps", "common", dir, tail @ ..]
                if *dir == "<game>" || ctx.game_dirs.iter().any(|d| d.eq_ignore_ascii_case(dir)) =>
            {
                via_root_common = true;
                ("{INSTALL_DIR}".to_string(), tail.join("/"))
            }
            ["userdata", "<storeUserId>", app, tail @ ..] => {
                let app = if *app == "<storeGameId>" {
                    match ctx.steam_id {
                        Some(id) => id.to_string(),
                        None => {
                            return Err((DropReason::Userdata, "no Steam id for <storeGameId>".into()));
                        }
                    }
                } else if !app.is_empty() && app.bytes().all(|b| b.is_ascii_digit()) {
                    app.to_string()
                } else {
                    return Err((DropReason::Userdata, format!("unknown userdata folder {app:?}")));
                };
                (format!("{{STEAM_USERDATA}}/{app}"), tail.join("/"))
            }
            // The whole account folder: mapped so the broad rule rejects it.
            ["userdata", "<storeUserId>"] => ("{STEAM_USERDATA}".to_string(), String::new()),
            _ => return Err(unsupported(format!("unknown <root> path {raw}"))),
        }
    } else if first.contains('<') {
        return Err(unsupported(format!("unresolvable placeholder {first}")));
    } else if first.is_empty() && path.starts_with('/') {
        // An absolute Unix path: kept as is.
        (String::new(), rest.to_string())
    } else {
        return Err(unsupported(format!("path isn't rooted at a known folder: {raw}")));
    };

    // Placeholders inside the rest of the path. Each may multiply the
    // spellings: `<storeUserId>` has two forms (rule 6), `<game>` one per
    // install folder name.
    let mut spellings = vec![rest];
    if spellings[0].contains("<storeUserId>") {
        if when.store.is_some_and(|s| s != Store::Steam) {
            return Err(unsupported("<storeUserId> of a store other than Steam".into()));
        }
        spellings = spellings
            .iter()
            .flat_map(|s| {
                [s.replace("<storeUserId>", "{STEAM_ID64}"), s.replace("<storeUserId>", "{STEAM_ACCOUNT_ID}")]
            })
            .collect();
    }
    if spellings[0].contains("<storeGameId>") {
        let id = match when.store {
            Some(Store::Gog) => ctx.gog_id,
            _ => ctx.steam_id,
        };
        let Some(id) = id else {
            return Err(unsupported("no store id for <storeGameId>".into()));
        };
        spellings = spellings.iter().map(|s| s.replace("<storeGameId>", &id.to_string())).collect();
    }
    if spellings[0].contains("<game>") {
        spellings = spellings.iter().flat_map(|s| ctx.game_dirs.iter().map(move |d| s.replace("<game>", d))).collect();
    }
    if let Some(start) = spellings[0].find('<')
        && let Some(len) = spellings[0][start..].find('>')
    {
        let token = &spellings[0][start..start + len + 1];
        let why = if ROOTS.iter().any(|(r, _)| *r == token) || token == "<root>" {
            format!("{token} occurs after the start of the path")
        } else {
            format!("unresolvable placeholder {token}")
        };
        return Err(unsupported(why));
    }

    let mut paths = Vec::new();
    for rest in spellings {
        let path = match (root.is_empty(), rest.is_empty()) {
            (true, _) => format!("/{rest}"),
            (false, true) => root.clone(),
            (false, false) => format!("{root}/{rest}"),
        };
        validate_template(&path).map_err(|e| unsupported(format!("{path}: {e}")))?;
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok(Mapped { paths, via_root_common })
}

/// `/` separators, no doubled or trailing slashes (rule 4).
fn clean(raw: &str) -> String {
    let mut path = raw.trim().replace('\\', "/");
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    path
}

/// A literal path whose last name has no extension: probably a whole folder.
fn looks_like_folder(path: &str) -> bool {
    let last = path.rsplit('/').next().unwrap_or(path);
    !path.split('/').any(has_wildcard) && !last.contains('.')
}

/// The segments of a template up to (not including) its first wildcard.
fn fixed_segments(path: &str) -> Vec<String> {
    path.split('/').take_while(|s| !has_wildcard(s)).map(|s| s.to_lowercase()).collect()
}

/// Whether a config entry sits strictly inside a save target's fixed part
/// and the two can apply together (rule 2).
fn inside(config: &PathRule, target: &PathRule) -> bool {
    fn compatible<T: PartialEq>(a: Option<T>, b: Option<T>) -> bool {
        a.is_none() || b.is_none() || a == b
    }
    if !compatible(config.when.os, target.when.os) || !compatible(config.when.store, target.when.store) {
        return false;
    }
    let fixed = fixed_segments(&target.path);
    let config_segments: Vec<String> = config.path.split('/').map(|s| s.to_lowercase()).collect();
    config_segments.len() > fixed.len() && config_segments[..fixed.len()] == fixed[..]
}

/// Tidies a rule list without reordering what survives:
/// - a path listed for every OS under one store becomes one rule without
///   `os` (Dead Cells' `windows`/`mac`/`linux` → no condition);
/// - a rule another rule for the same path already covers is dropped
///   (`{os: windows, store: steam}` next to `{os: windows}`);
/// - exact duplicates are dropped.
pub fn normalize(rules: Vec<PathRule>) -> Vec<PathRule> {
    let mut rules = rules;
    // Collapse all-OS groups.
    let mut i = 0;
    while i < rules.len() {
        let PathRule { when, path } = rules[i].clone();
        if when.os.is_some() {
            let has = |os| rules.iter().any(|r| r.path == path && r.when == When { os: Some(os), store: when.store });
            if Platform::ALL.iter().all(|&os| has(os)) {
                rules[i].when.os = None;
                let store = when.store;
                let mut j = 0;
                rules.retain(|r| {
                    let keep = j == i || !(r.path == path && r.when.store == store && r.when.os.is_some());
                    j += 1;
                    keep
                });
            }
        }
        i += 1;
    }
    // Drop covered rules and duplicates, keeping the first occurrence.
    let covers =
        |a: &When, b: &When| a.os.is_none_or(|os| b.os == Some(os)) && a.store.is_none_or(|s| b.store == Some(s));
    let mut out: Vec<PathRule> = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        let covered = rules.iter().enumerate().any(|(other, r)| {
            other != index && r.path == rule.path && r.when != rule.when && covers(&r.when, &rule.when)
        });
        if !covered && !out.contains(rule) {
            out.push(rule.clone());
        }
    }
    out
}

/// `launch` → `executables` (3.1).
fn translate_launch(game: &ManifestGame, out: &mut Translation) {
    for (raw, entries) in &game.launch {
        let path = clean(raw);
        let Some(relative) = path.strip_prefix("<base>/") else {
            out.dropped_launch.push((raw.clone(), "not under <base>".into()));
            continue;
        };
        if relative.contains('<') || relative.contains('{') {
            out.dropped_launch.push((raw.clone(), "unresolvable placeholder".into()));
            continue;
        }
        if !is_program(relative) {
            out.dropped_launch.push((raw.clone(), "not a program".into()));
            continue;
        }
        // Absent `os` (or no `when` at all) means every OS; `bit` and
        // `store` are ignored.
        let mut platforms = BTreeSet::new();
        let conditions: Vec<&Condition> = entries.iter().flat_map(|e| e.when.iter()).collect();
        let unconditional = entries.is_empty() || entries.iter().any(|e| e.when.is_empty());
        if unconditional {
            platforms.extend(Platform::ALL);
        }
        for condition in conditions {
            match condition.os.as_deref() {
                None => platforms.extend(Platform::ALL),
                Some("windows") => {
                    platforms.insert(Platform::Windows);
                }
                Some("mac") => {
                    platforms.insert(Platform::Macos);
                }
                Some("linux") => {
                    platforms.insert(Platform::Linux);
                }
                Some(_) => {}
            }
        }
        for platform in platforms {
            let list = out.entry.executables.for_platform_mut(platform);
            if !list.iter().any(|e| e == relative) {
                list.push(relative.to_string());
            }
        }
    }
}

/// Launch targets that are documents, links or media rather than programs
/// (Total War: Shogun 2 lists `data/encyclopedia/how_to_play.html`). Every
/// build ships such files, so as executables they'd make every install look
/// like every build.
const NOT_PROGRAMS: &[&str] = &[
    "htm", "html", "pdf", "txt", "rtf", "md", "doc", "docx", "chm", "url", "lnk", "png", "jpg", "jpeg", "gif", "bmp",
    "ttf", "mp4", "m4v", "avi", "zip", "7z", "rar", "dmg", "msi", "ini", "cfg", "xml", "json", "log",
];

fn is_program(relative: &str) -> bool {
    let last = relative.rsplit('/').next().unwrap_or(relative);
    match last.rsplit_once('.') {
        Some((_, extension)) => !NOT_PROGRAMS.contains(&extension.to_ascii_lowercase().as_str()),
        None => true,
    }
}

fn dedupe(ids: &mut Vec<u64>) {
    let mut seen = BTreeSet::new();
    ids.retain(|id| seen.insert(*id));
}
