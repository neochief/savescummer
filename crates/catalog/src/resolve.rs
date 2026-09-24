//! The resolver: builds each install's save set from a catalog entry
//! (PLAN-CATALOG.md Section 4). Pure by construction: every observation of
//! the machine goes through [`Probe`].

use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::broad::is_broad_template;
use crate::glob::{has_wildcard, match_path, match_segment};
use crate::model::{Game, PathRule, Platform, Store};

/// Whether a path exists. Only `Missing` counts as absent: a folder on an
/// unplugged drive is `Unknown`, never missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Presence {
    Present,
    Missing,
    Unknown,
}

/// What inside a target's root counts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Filter {
    /// Everything in the root.
    All,
    /// One entry, file or folder, by its exact name.
    Exact(String),
    /// A relative `/`-separated glob.
    Pattern(String),
}

impl Filter {
    pub fn describe(&self) -> String {
        match self {
            Filter::All => "*everything*".into(),
            Filter::Exact(name) => name.clone(),
            Filter::Pattern(p) => p.clone(),
        }
    }

    /// Two filters are the same filter under the file system's case rules.
    pub fn same(&self, other: &Filter, case_insensitive: bool) -> bool {
        match (self, other) {
            (Filter::All, Filter::All) => true,
            (Filter::Exact(a), Filter::Exact(b)) | (Filter::Pattern(a), Filter::Pattern(b)) => {
                if case_insensitive {
                    a.to_lowercase() == b.to_lowercase()
                } else {
                    a == b
                }
            }
            _ => false,
        }
    }

    /// Whether a root-relative path (segments) is matched by this filter,
    /// either itself or as something inside a matched entry.
    pub fn covers(&self, relative: &[&str], case_insensitive: bool) -> bool {
        match self {
            Filter::All => !relative.is_empty(),
            Filter::Exact(name) => relative.first().is_some_and(|first| eq_name(first, name, case_insensitive)),
            Filter::Pattern(pattern) => {
                (1..=relative.len()).any(|n| match_path(pattern, &relative[..n].join("/"), case_insensitive))
            }
        }
    }
}

pub fn eq_name(a: &str, b: &str, case_insensitive: bool) -> bool {
    if case_insensitive { a.to_lowercase() == b.to_lowercase() } else { a == b }
}

/// One place a game keeps saves: a real root folder plus a filter, minus
/// excludes (root-relative `/`-separated globs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub root: PathBuf,
    pub filter: Filter,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excludes: Vec<String>,
    pub presence: Presence,
}

impl Target {
    /// The path a target claims: the named entry for an exact name, the root
    /// otherwise. Overlap and coverage are judged on it.
    pub fn claim(&self) -> PathBuf {
        match &self.filter {
            Filter::Exact(name) => self.root.join(name),
            _ => self.root.clone(),
        }
    }
}

/// OS folders placeholders resolve to on this machine, for native builds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownFolders {
    pub home: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub localappdata: Option<PathBuf>,
    pub locallow: Option<PathBuf>,
    pub documents: Option<PathBuf>,
    pub public: Option<PathBuf>,
    pub programdata: Option<PathBuf>,
    pub programfiles: Option<PathBuf>,
    pub programfiles_x86: Option<PathBuf>,
    pub windir: Option<PathBuf>,
    pub saved_games: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    /// The main Steam folder, which holds `userdata`.
    pub steam_root: Option<PathBuf>,
}

/// The current Steam account. Both id forms come from one number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SteamAccount {
    pub account_id: u32,
}

pub const STEAM_ID64_BASE: u64 = 76_561_197_960_265_728;

impl SteamAccount {
    pub fn from_id64(id64: u64) -> Option<SteamAccount> {
        id64.checked_sub(STEAM_ID64_BASE)
            .and_then(|id| u32::try_from(id).ok())
            .filter(|id| *id != 0)
            .map(|account_id| SteamAccount { account_id })
    }

    pub fn id64(self) -> u64 {
        STEAM_ID64_BASE + u64::from(self.account_id)
    }
}

/// Every observation the resolver makes of the machine.
pub trait Probe {
    fn presence(&self, path: &Path) -> Presence;
    fn is_file(&self, path: &Path) -> bool;
    /// Real-directory equality: case rules, links, redirected folders.
    fn same_dir(&self, a: &Path, b: &Path) -> bool;
    /// Tells apart two copies of one product that exist at the same time.
    fn install_identity(&self, install_dir: &Path) -> Option<String>;
    fn steam_account(&self) -> Option<SteamAccount>;
    fn folders(&self) -> KnownFolders;
    /// Folder names directly inside a folder (for the Proton profile).
    fn list_dirs(&self, path: &Path) -> Option<Vec<String>>;
}

/// One install found by discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Install {
    pub catalog_id: String,
    pub store: Store,
    /// The machine: windows, macos or linux.
    pub os: Platform,
    pub install_dir: PathBuf,
    /// Where a Proton prefix for this install lives (Steam on Linux),
    /// whether or not it exists yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proton_prefix: Option<PathBuf>,
}

/// What a save set was resolved in. A different context gives a different
/// save set; old checkpoints stay recorded against theirs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub builds: Vec<Platform>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_account: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Outcome {
    Resolved { save_set: Vec<Target> },
    Unsupported { reason: String },
}

/// The resolver's answer for one install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub catalog_id: String,
    pub install: Install,
    /// Absolute executables of every possible build, for the monitor.
    pub executables: Vec<PathBuf>,
    pub context: Context,
    pub outcome: Outcome,
    pub warnings: Vec<String>,
}

impl Decision {
    pub fn save_set(&self) -> Option<&[Target]> {
        match &self.outcome {
            Outcome::Resolved { save_set } => Some(save_set),
            Outcome::Unsupported { .. } => None,
        }
    }
}

/// Decides which builds an install may run (PLAN-CATALOG.md 4.3).
pub fn decide_builds(game: &Game, install: &Install, probe: &dyn Probe) -> Vec<Platform> {
    match install.os {
        Platform::Windows => vec![Platform::Windows],
        Platform::Macos => vec![Platform::Macos],
        Platform::Linux => {
            if install.proton_prefix.is_none() {
                return vec![Platform::Linux];
            }
            let windows = &game.executables.windows;
            let linux = &game.executables.linux;
            if windows.is_empty() || linux.is_empty() {
                return vec![Platform::Linux, Platform::Windows];
            }
            let shared = windows.iter().any(|w| linux.contains(w));
            if shared {
                return vec![Platform::Linux, Platform::Windows];
            }
            let exists = |list: &[String]| list.iter().any(|exe| probe.is_file(&install.install_dir.join(exe)));
            match (exists(windows), exists(linux)) {
                (true, false) => vec![Platform::Windows],
                (false, true) => vec![Platform::Linux],
                _ => vec![Platform::Linux, Platform::Windows],
            }
        }
    }
}

/// Placeholder values for one build of one install.
struct Bases {
    folders: KnownFolders,
    install_dir: PathBuf,
    account: Option<SteamAccount>,
}

impl Bases {
    fn for_build(build: Platform, install: &Install, probe: &dyn Probe, account: Option<SteamAccount>) -> Bases {
        let native = probe.folders();
        let folders = if build == Platform::Windows && install.os != Platform::Windows {
            proton_folders(install, probe, &native)
        } else {
            native
        };
        Bases { folders, install_dir: install.install_dir.clone(), account }
    }

    fn base(&self, placeholder: &str, build: Platform) -> Result<PathBuf, String> {
        let f = &self.folders;
        let windows_only = |value: &Option<PathBuf>| -> Result<PathBuf, String> {
            if build != Platform::Windows {
                return Err(format!("{{{placeholder}}} doesn't apply to a {build} build"));
            }
            value.clone().ok_or_else(|| format!("{{{placeholder}}} is unknown on this machine"))
        };
        let linux_only = |value: &Option<PathBuf>| -> Result<PathBuf, String> {
            if build != Platform::Linux {
                return Err(format!("{{{placeholder}}} doesn't apply to a {build} build"));
            }
            value.clone().ok_or_else(|| format!("{{{placeholder}}} is unknown on this machine"))
        };
        match placeholder {
            "INSTALL_DIR" => Ok(self.install_dir.clone()),
            "HOME" => f.home.clone().ok_or_else(|| "the home folder is unknown".into()),
            "APPDATA" => windows_only(&f.appdata),
            "LOCALAPPDATA" => windows_only(&f.localappdata),
            "LOCALLOW" => windows_only(&f.locallow),
            "DOCUMENTS" => windows_only(&f.documents),
            "PUBLIC" => windows_only(&f.public),
            "PROGRAMDATA" => windows_only(&f.programdata),
            "PROGRAMFILES" => windows_only(&f.programfiles),
            "WINDIR" => windows_only(&f.windir),
            "XDG_DATA_HOME" => linux_only(&f.xdg_data_home),
            "XDG_CONFIG_HOME" => linux_only(&f.xdg_config_home),
            "STEAM_USERDATA" => {
                let account = self.account.ok_or("the Steam account is unknown")?;
                let root = f.steam_root.clone().ok_or("Steam isn't installed")?;
                Ok(root.join("userdata").join(account.account_id.to_string()))
            }
            other => Err(format!("{{{other}}} can't start a path")),
        }
    }

    fn inline(&self, segment: &str) -> Result<String, String> {
        if !segment.contains('{') {
            return Ok(segment.to_string());
        }
        let mut out = segment.to_string();
        for (name, value) in [
            ("{STEAM_ID64}", self.account.map(|a| a.id64().to_string())),
            ("{STEAM_ACCOUNT_ID}", self.account.map(|a| a.account_id.to_string())),
        ] {
            if out.contains(name) {
                let value = value.ok_or("the Steam account is unknown")?;
                out = out.replace(name, &value);
            }
        }
        if out.contains('{') {
            return Err(format!("{segment:?} uses a placeholder that can't appear inside a path"));
        }
        Ok(out)
    }

    /// Resolves a template into a base folder and the segments below it.
    fn resolve(&self, template: &str, build: Platform) -> Result<(PathBuf, Vec<String>), String> {
        let mut segments = template.split('/').filter(|s| !s.is_empty());
        let base = if template.starts_with('/') {
            PathBuf::from("/")
        } else {
            let first = segments.next().ok_or("empty path")?;
            let name = first
                .strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .ok_or_else(|| format!("{template:?} must start with a placeholder"))?;
            self.base(name, build)?
        };
        let rest = segments.map(|s| self.inline(s)).collect::<Result<Vec<_>, _>>()?;
        Ok((base, rest))
    }
}

/// Windows placeholders inside a Proton prefix (PLAN-CATALOG.md 4.4).
fn proton_folders(install: &Install, probe: &dyn Probe, native: &KnownFolders) -> KnownFolders {
    let Some(prefix) = &install.proton_prefix else {
        return KnownFolders { steam_root: native.steam_root.clone(), ..Default::default() };
    };
    let drive_c = prefix.join("drive_c");
    let users = drive_c.join("users");
    let profile_name = probe
        .list_dirs(&users)
        .map(|names| {
            let others: Vec<String> =
                names.into_iter().filter(|n| n != "steamuser" && !n.eq_ignore_ascii_case("public")).collect();
            if others.len() == 1 { others[0].clone() } else { "steamuser".to_string() }
        })
        .unwrap_or_else(|| "steamuser".to_string());
    let profile = users.join(profile_name);
    KnownFolders {
        home: Some(profile.clone()),
        appdata: Some(profile.join("AppData").join("Roaming")),
        localappdata: Some(profile.join("AppData").join("Local")),
        locallow: Some(profile.join("AppData").join("LocalLow")),
        documents: Some(profile.join("Documents")),
        saved_games: Some(profile.join("Saved Games")),
        public: Some(users.join("Public")),
        programdata: Some(drive_c.join("ProgramData")),
        programfiles: Some(drive_c.join("Program Files")),
        programfiles_x86: Some(drive_c.join("Program Files (x86)")),
        windir: Some(drive_c.join("windows")),
        xdg_data_home: None,
        xdg_config_home: None,
        steam_root: native.steam_root.clone(),
    }
}

/// Builds one install's save set from its catalog entry.
pub fn resolve(game: &Game, install: &Install, probe: &dyn Probe) -> Decision {
    let builds = decide_builds(game, install, probe);
    let account = probe.steam_account();
    let case_insensitive = install.os.case_insensitive();
    let mut warnings = Vec::new();
    let mut targets: Vec<Target> = Vec::new();
    let mut excludes: Vec<(PathBuf, Vec<String>)> = Vec::new();

    for &build in &builds {
        let bases = Bases::for_build(build, install, probe, account);
        for rule in applicable(&game.save, build, install.store) {
            if is_broad_template(&rule.path) {
                warnings.push(format!("{}: takes a broad folder; left out", rule.path));
                continue;
            }
            match bases.resolve(&rule.path, build) {
                Ok((base, segments)) => {
                    if let Some(target) = split_target(base, &segments) {
                        push_target(&mut targets, target, probe, case_insensitive);
                    }
                }
                Err(why) => warnings.push(format!("{}: {why}; left out", rule.path)),
            }
        }
        for rule in applicable(&game.exclude, build, install.store) {
            if let Ok((base, segments)) = bases.resolve(&rule.path, build) {
                excludes.push((base, segments));
            }
        }
    }

    // Drop targets another target of this game covers entirely.
    let mut kept: Vec<Target> = Vec::new();
    for (i, target) in targets.iter().enumerate() {
        let covered =
            targets.iter().enumerate().any(|(j, other)| i != j && covers(other, target, probe, case_insensitive));
        if !covered {
            kept.push(target.clone());
        }
    }

    for (base, segments) in &excludes {
        let full = join_segments(base.clone(), segments);
        for target in &mut kept {
            if let Some(relative) = relative_to(&full, &target.root, probe, case_insensitive) {
                let parts: Vec<&str> = relative.iter().map(String::as_str).collect();
                if target.filter.covers(&parts, case_insensitive) {
                    let text = relative.join("/");
                    if !target.excludes.contains(&text) {
                        target.excludes.push(text);
                    }
                }
            }
        }
    }

    for target in &mut kept {
        target.presence = probe.presence(&target.root);
    }

    let mut executables = Vec::new();
    for &build in &builds {
        for exe in game.executables.for_platform(build) {
            if exe.ends_with(".sh") {
                continue;
            }
            let path = install.install_dir.join(exe);
            if !executables.contains(&path) {
                executables.push(path);
            }
        }
    }

    let outcome = if kept.is_empty() {
        let builds_text: Vec<&str> = builds.iter().map(|b| b.as_str()).collect();
        Outcome::Unsupported {
            reason: format!(
                "the catalog has no save location for the {} build from {}",
                builds_text.join("/"),
                install.store.display_name()
            ),
        }
    } else {
        Outcome::Resolved { save_set: kept }
    };

    Decision {
        catalog_id: game.id.clone(),
        install: install.clone(),
        executables,
        context: Context { builds, steam_account: account.map(|a| a.account_id) },
        outcome,
        warnings,
    }
}

fn applicable(rules: &[PathRule], build: Platform, store: Store) -> impl Iterator<Item = &PathRule> {
    rules.iter().filter(move |r| r.applies(build, store))
}

fn join_segments(mut base: PathBuf, segments: &[String]) -> PathBuf {
    for s in segments {
        base.push(s);
    }
    base
}

/// Splits a resolved path into root and filter: the root is everything
/// before the first wildcard segment; a literal path is its parent plus the
/// exact last name.
pub fn split_target(base: PathBuf, segments: &[String]) -> Option<Target> {
    let (root, filter) = match segments.iter().position(|s| has_wildcard(s)) {
        Some(first) => {
            let root = join_segments(base, &segments[..first]);
            (root, Filter::Pattern(segments[first..].join("/")))
        }
        None => {
            let (last, parents) = segments.split_last()?;
            (join_segments(base, parents), Filter::Exact(last.clone()))
        }
    };
    Some(Target { root, filter, excludes: Vec::new(), presence: Presence::Unknown })
}

/// Splits a user-given absolute location (folder, file or glob) into a
/// target, the way catalog paths are split.
pub fn split_location(location: &Path) -> Option<Target> {
    let mut base = PathBuf::new();
    let mut segments = Vec::new();
    for component in location.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => base.push(component.as_os_str()),
            Component::Normal(name) => segments.push(name.to_string_lossy().into_owned()),
            Component::CurDir | Component::ParentDir => return None,
        }
    }
    split_target(base, &segments)
}

fn push_target(targets: &mut Vec<Target>, target: Target, probe: &dyn Probe, ci: bool) {
    let duplicate = targets.iter().any(|t| t.filter.same(&target.filter, ci) && probe.same_dir(&t.root, &target.root));
    if !duplicate {
        targets.push(target);
    }
}

/// Whether `a` covers every file `b` could match.
fn covers(a: &Target, b: &Target, probe: &dyn Probe, ci: bool) -> bool {
    if a.filter.same(&b.filter, ci) && probe.same_dir(&a.root, &b.root) {
        return false; // duplicates were already removed; never drop both
    }
    // The folder whose whole content `a` takes.
    let folder = match &a.filter {
        Filter::All => a.root.clone(),
        Filter::Exact(_) => a.claim(),
        Filter::Pattern(_) => return false,
    };
    b.root.ancestors().any(|ancestor| probe.same_dir(ancestor, &folder))
}

/// `path` relative to `root` as segments, when it's inside it.
fn relative_to(path: &Path, root: &Path, probe: &dyn Probe, ci: bool) -> Option<Vec<String>> {
    let path_parts: Vec<String> = normal_parts(path);
    let root_parts: Vec<String> = normal_parts(root);
    if path_parts.len() > root_parts.len() && path_parts.iter().zip(&root_parts).all(|(a, b)| eq_name(a, b, ci)) {
        return Some(path_parts[root_parts.len()..].to_vec());
    }
    // Different spelling of the same folder (a link or a redirect).
    for (depth, ancestor) in path.ancestors().enumerate().skip(1) {
        if probe.same_dir(ancestor, root) {
            let parts = normal_parts(path);
            return Some(parts[parts.len() - depth..].to_vec());
        }
    }
    None
}

fn normal_parts(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// One game record: an install, or several merged because their save sets
/// share a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameRecord {
    /// Install-level id: `steam-588650`, `steam-588650#gog`,
    /// `steam-588650#<identity>`.
    pub id: String,
    pub catalog_id: String,
    pub installs: Vec<Install>,
    pub install_identities: Vec<Option<String>>,
    pub executables: Vec<PathBuf>,
    pub context: Context,
    pub outcome: Outcome,
    pub warnings: Vec<String>,
}

fn store_rank(store: Store) -> u8 {
    match store {
        Store::Steam => 0,
        Store::Gog => 1,
        Store::Epic => 2,
        Store::Standalone => 3,
    }
}

/// The store a catalog id is named after, if any.
fn primary_store(catalog_id: &str) -> Option<Store> {
    if catalog_id.starts_with("steam-") {
        Some(Store::Steam)
    } else if catalog_id.starts_with("gog-") {
        Some(Store::Gog)
    } else {
        None
    }
}

/// Assigns install-level ids and merges installs whose save sets share a
/// target (PLAN-CATALOG.md 4.2).
///
/// `known` maps previously assigned ids to their install identity, so of
/// two simultaneous copies in one store the one seen before keeps its id.
pub fn assign_games(decisions: &[Decision], probe: &dyn Probe, known: &HashMap<String, String>) -> Vec<GameRecord> {
    let mut by_catalog: Vec<(String, Vec<&Decision>)> = Vec::new();
    for decision in decisions {
        match by_catalog.iter_mut().find(|(id, _)| *id == decision.catalog_id) {
            Some((_, list)) => list.push(decision),
            None => by_catalog.push((decision.catalog_id.clone(), vec![decision])),
        }
    }

    let mut records = Vec::new();
    for (catalog_id, mut group) in by_catalog {
        let identities: HashMap<*const Decision, Option<String>> =
            group.iter().map(|d| (*d as *const Decision, probe.install_identity(&d.install.install_dir))).collect();
        let identity = |d: &Decision| identities[&(d as *const Decision)].clone();
        let primary = primary_store(&catalog_id);
        group.sort_by(|a, b| {
            let rank = |d: &Decision| {
                let primary_first = if Some(d.install.store) == primary { 0 } else { 1 };
                let previously_bare = known.get(&catalog_id).is_some_and(|i| Some(i.clone()) == identity(d));
                (primary_first, store_rank(d.install.store), !previously_bare)
            };
            rank(a).cmp(&rank(b)).then_with(|| a.install.install_dir.cmp(&b.install.install_dir))
        });

        let mut used_stores = BTreeSet::new();
        let mut assigned: Vec<GameRecord> = Vec::new();
        for (index, decision) in group.iter().enumerate() {
            let store = decision.install.store;
            let id = if index == 0 {
                catalog_id.clone()
            } else if !used_stores.contains(&store) {
                format!("{catalog_id}#{}", store.as_str())
            } else {
                let tag =
                    identity(decision).unwrap_or_else(|| decision.install.install_dir.to_string_lossy().to_lowercase());
                format!("{catalog_id}#{}", sanitize_tag(&tag))
            };
            used_stores.insert(store);
            assigned.push(GameRecord {
                id,
                catalog_id: catalog_id.clone(),
                installs: vec![decision.install.clone()],
                install_identities: vec![identity(decision)],
                executables: decision.executables.clone(),
                context: decision.context.clone(),
                outcome: decision.outcome.clone(),
                warnings: decision.warnings.clone(),
            });
        }

        // Merge records whose save sets share a target: a folder carries one
        // checkpoint history.
        let ci = group.first().map(|d| d.install.os.case_insensitive()).unwrap_or(true);
        let mut merged: Vec<GameRecord> = Vec::new();
        for record in assigned {
            let position = merged.iter().position(|m| share_target(m, &record, probe, ci));
            match position {
                Some(i) => merge_into(&mut merged[i], record, probe, ci),
                None => merged.push(record),
            }
        }
        records.extend(merged);
    }
    records
}

fn sanitize_tag(tag: &str) -> String {
    tag.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

fn share_target(a: &GameRecord, b: &GameRecord, probe: &dyn Probe, ci: bool) -> bool {
    let (Outcome::Resolved { save_set: sa }, Outcome::Resolved { save_set: sb }) = (&a.outcome, &b.outcome) else {
        return false;
    };
    sa.iter().any(|x| sb.iter().any(|y| x.filter.same(&y.filter, ci) && probe.same_dir(&x.root, &y.root)))
}

fn merge_into(into: &mut GameRecord, other: GameRecord, probe: &dyn Probe, ci: bool) {
    into.installs.extend(other.installs);
    into.install_identities.extend(other.install_identities);
    for exe in other.executables {
        if !into.executables.contains(&exe) {
            into.executables.push(exe);
        }
    }
    for build in other.context.builds {
        if !into.context.builds.contains(&build) {
            into.context.builds.push(build);
        }
    }
    into.warnings.extend(other.warnings);
    if let (Outcome::Resolved { save_set }, Outcome::Resolved { save_set: theirs }) = (&mut into.outcome, other.outcome)
    {
        for target in theirs {
            push_target(save_set, target, probe, ci);
        }
    }
}

/// Lowercases on case-insensitive platforms, for comparisons of names.
pub fn fold_name(name: &str, case_insensitive: bool) -> String {
    if case_insensitive { name.to_lowercase() } else { name.to_string() }
}

/// Whether a root-relative entry name matches a single segment pattern.
pub fn segment_matches(pattern: &str, name: &str, case_insensitive: bool) -> bool {
    match_segment(pattern, name, case_insensitive)
}
