//! The game library: running discovery and the catalog resolver, keeping
//! game records, and deriving each game's validated save set. The catalog
//! decides where a known game's saves are; the host only applies the same
//! safety rules to every target.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use savescummer_catalog::{Decision, Install, Outcome, Probe, assign_games, resolve, split_location};
use savescummer_core::safety::{OtherGame, RealPaths, SafetyInput, check};
use savescummer_core::{ErrorKind, Failure, Presence, Target};
use savescummer_ipc::GameKind;
use savescummer_scanner::{RealProbe, discover};
use savescummer_storage as db;

use crate::host::{Host, Inner};
use crate::model::{Derived, Game, SETTING_COUNTER, UserLocation};

/// Real paths, resolved once per pass.
pub struct CachedPaths {
    ci: bool,
    cache: RefCell<HashMap<PathBuf, Result<PathBuf, String>>>,
}

impl CachedPaths {
    pub fn new(ci: bool) -> CachedPaths {
        CachedPaths { ci, cache: RefCell::new(HashMap::new()) }
    }
}

impl RealPaths for CachedPaths {
    fn real(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(hit) = self.cache.borrow().get(path) {
            return hit.clone();
        }
        let value = savescummer_snapshots::real_path(path);
        self.cache.borrow_mut().insert(path.to_path_buf(), value.clone());
        value
    }

    fn case_insensitive(&self) -> bool {
        self.ci
    }
}

/// A game's targets before validation: its user location, or the catalog's
/// resolved save set.
fn raw_targets(game: &Game) -> Result<(Vec<Target>, bool), Failure> {
    if let Some(location) = &game.location {
        let target = split_location(Path::new(&location.text))
            .ok_or_else(|| Failure::new(ErrorKind::InvalidConfig, "use a full path").path(&location.text))?;
        return Ok((vec![target], true));
    }
    match &game.outcome {
        Some(Outcome::Resolved { save_set }) => Ok((save_set.clone(), false)),
        Some(Outcome::Unsupported { reason }) => Err(Failure::new(ErrorKind::NoSaveLocation, reason.clone())),
        None => Err(Failure::new(ErrorKind::NoSaveLocation, "the game has no save location")),
    }
}

/// Recomputes every game's validated save set, presence and data.
pub fn derive_all(host: &Host, inner: &mut Inner) {
    let paths = CachedPaths::new(host.env.case_insensitive());
    let broad = host.env.broad_folders();
    let raw: BTreeMap<String, Result<(Vec<Target>, bool), Failure>> =
        inner.games.iter().map(|(id, g)| (id.clone(), raw_targets(g))).collect();
    let names: HashMap<String, String> = inner.games.iter().map(|(id, g)| (id.clone(), g.name.clone())).collect();
    let mut derived = HashMap::new();
    for (id, game) in &inner.games {
        derived.insert(id.clone(), derive(host, game, &raw, &names, &broad, &paths));
    }
    inner.derived = derived;
}

/// Recomputes one game (before an operation, after a configuration change).
pub fn derive_one(host: &Host, inner: &mut Inner, game_id: &str) {
    let paths = CachedPaths::new(host.env.case_insensitive());
    let broad = host.env.broad_folders();
    let raw: BTreeMap<String, Result<(Vec<Target>, bool), Failure>> =
        inner.games.iter().map(|(id, g)| (id.clone(), raw_targets(g))).collect();
    let names: HashMap<String, String> = inner.games.iter().map(|(id, g)| (id.clone(), g.name.clone())).collect();
    if let Some(game) = inner.games.get(game_id) {
        let value = derive(host, game, &raw, &names, &broad, &paths);
        inner.derived.insert(game_id.to_string(), value);
    }
}

fn derive(
    host: &Host,
    game: &Game,
    raw: &BTreeMap<String, Result<(Vec<Target>, bool), Failure>>,
    names: &HashMap<String, String>,
    broad: &[PathBuf],
    paths: &CachedPaths,
) -> Derived {
    let mut warnings = game.warnings.clone();
    let (targets, user) = match &raw[&game.id] {
        Ok(value) => value.clone(),
        Err(failure) => {
            return Derived { active: Err(failure.clone().game(&game.id)), has_data: false, warnings, access: None };
        }
    };
    // Waiting for macOS to allow access: nothing inside is touched.
    let roots: Vec<PathBuf> = targets.iter().map(|t| t.root.clone()).collect();
    if let Some((path, category)) = crate::privacy::game_needs(host, &roots, &game.install_dirs()) {
        let failure = Failure::new(ErrorKind::AccessNeeded, category.as_str()).path(&path).game(&game.id);
        return Derived { active: Err(failure), has_data: false, warnings, access: Some((path, category)) };
    }
    // Other games' targets: every user location, and the catalog targets of
    // games ordered before this one, so two catalog games never both drop a
    // shared target.
    let others_owned: Vec<(String, String, Vec<Target>)> = raw
        .iter()
        .filter(|(id, _)| *id != &game.id)
        .filter_map(|(id, r)| {
            let (targets, is_user) = r.as_ref().ok()?;
            (*is_user || id.as_str() < game.id.as_str()).then(|| {
                let name = names.get(id).cloned().unwrap_or_else(|| id.clone());
                (id.clone(), name, targets.clone())
            })
        })
        .collect();
    let others: Vec<OtherGame<'_>> =
        others_owned.iter().map(|(id, name, targets)| OtherGame { id, name, targets }).collect();
    let install_dirs = game.install_dirs();
    let executables = game.executables();
    let check_all = |targets: &[Target]| -> Result<Vec<Target>, Failure> {
        // Every install folder is broad for this game's targets.
        let mut checked = targets.to_vec();
        let dirs: Vec<Option<&Path>> =
            if install_dirs.is_empty() { vec![None] } else { install_dirs.iter().map(|d| Some(d.as_path())).collect() };
        for dir in dirs {
            let input = SafetyInput {
                game_id: &game.id,
                targets: &checked,
                broad,
                install_dir: dir,
                executables: &executables,
                others: &others,
            };
            checked = check(&input, paths)?;
        }
        Ok(checked)
    };

    let active = if user {
        // A link that now points elsewhere fails until configured again.
        if let (Some(location), Some(target)) = (&game.location, targets.first()) {
            let now = paths.real(&target.root).unwrap_or_else(|_| target.root.clone());
            if !savescummer_core::common::same_path(&now, &location.real_root, paths.case_insensitive()) {
                let failure = Failure::new(
                    ErrorKind::InvalidTarget,
                    "the save location is a link that changed or can't be resolved",
                )
                .path(&target.root)
                .game(&game.id);
                return Derived { active: Err(failure), has_data: false, warnings, access: None };
            }
        }
        check_all(&targets)
    } else {
        let mut valid = Vec::new();
        for target in &targets {
            match check_all(std::slice::from_ref(target)) {
                Ok(mut checked) => valid.append(&mut checked),
                Err(failure) => warnings.push(format!("{}: {}; left out", target.root.display(), failure.detail)),
            }
        }
        if valid.is_empty() {
            Err(Failure::new(ErrorKind::NoSaveLocation, "no valid save location").game(&game.id))
        } else {
            Ok(valid)
        }
    };

    // A location that turned unreadable may be macOS taking access back.
    if let Ok(targets) = &active
        && targets
            .iter()
            .any(|t| savescummer_snapshots::presence(&t.root) == Presence::Unknown && host.privacy.taken_back(&t.root))
    {
        return derive(host, game, raw, names, broad, paths);
    }
    let mut has_data = false;
    let active = active.map(|targets| {
        targets
            .into_iter()
            .map(|mut t| {
                t.presence = savescummer_snapshots::presence(&t.root);
                if t.presence == Presence::Present {
                    match savescummer_snapshots::walk_target(&t.root, &t.filter, &t.excludes, paths.case_insensitive())
                    {
                        Ok(entries) => has_data |= !entries.is_empty(),
                        // A link or unreadable file: let Save report it.
                        Err(_) => has_data = true,
                    }
                }
                t
            })
            .collect()
    });
    Derived { active, has_data, warnings, access: None }
}

/// Refreshes presence and data for some games (running games, after an
/// operation) without re-validating. Returns whether anything changed.
pub fn refresh_presence(host: &Host, inner: &mut Inner, games: &[String]) -> bool {
    let ci = host.env.case_insensitive();
    let mut changed = false;
    for id in games {
        let Some(derived) = inner.derived.get_mut(id) else { continue };
        let Ok(targets) = &mut derived.active else { continue };
        let mut has_data = false;
        for t in targets.iter_mut() {
            let presence = savescummer_snapshots::presence(&t.root);
            changed |= presence != t.presence;
            t.presence = presence;
            if presence == Presence::Present {
                has_data |= savescummer_snapshots::walk_target(&t.root, &t.filter, &t.excludes, ci)
                    .map(|e| !e.is_empty())
                    .unwrap_or(true);
            }
        }
        changed |= has_data != derived.has_data;
        derived.has_data = has_data;
    }
    changed
}

/// Runs one scan: discovery, the resolver, and merging into the library.
/// Returns how many known games were found for the first time.
pub fn scan(host: &Arc<Host>, full: bool, reason: &str) -> usize {
    static SCANS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let bundle = host.bundle();
    // Where a slow or sleeping drive stalls a scan: reading store records.
    host.crash_point("scan.discover", SCANS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1);
    let found = discover(&bundle, &host.env);
    let probe = RealProbe::new(&host.env);
    let decisions: Vec<Decision> = found
        .installs
        .iter()
        .filter_map(|install| bundle.game(&install.catalog_id).map(|game| resolve(game, install, &probe)))
        .collect();
    let known: HashMap<String, String> = {
        let inner = host.lock();
        inner.games.values().filter_map(|g| Some((g.id.clone(), g.identities.first()?.clone()?))).collect()
    };
    let records = assign_games(&decisions, &probe, &known);
    let ci = host.env.case_insensitive();

    let mut inner = host.lock();
    let before: HashMap<String, bool> = inner.games.iter().map(|(id, g)| (id.clone(), g.installed)).collect();
    let mut new_games = 0;
    let mut seen = Vec::new();
    let mut counter = next_counter(host);
    for record in &records {
        let Some(entry) = bundle.game(&record.catalog_id) else { continue };
        seen.push(record.id.clone());
        let game = inner.games.entry(record.id.clone()).or_insert_with(|| {
            new_games += 1;
            counter += 1;
            Game {
                id: record.id.clone(),
                kind: GameKind::Known,
                catalog_id: Some(record.catalog_id.clone()),
                name: entry.name.clone(),
                info: None,
                installed: true,
                installs: Vec::new(),
                identities: Vec::new(),
                catalog_executables: Vec::new(),
                outcome: None,
                context: None,
                warnings: Vec::new(),
                install_tag: None,
                executable: None,
                location: None,
                created: counter,
            }
        });
        game.name = entry.name.clone();
        game.info = entry.info.clone();
        game.installed = true;
        game.installs = record.installs.clone();
        game.identities = record.install_identities.clone();
        game.catalog_executables = record.executables.clone();
        game.outcome = Some(record.outcome.clone());
        game.context = Some(record.context.clone());
        game.warnings = record.warnings.clone();
    }
    // Known games not found: uninstalled only when their absence is
    // confirmed. An unreadable library keeps the previous state.
    for game in inner.games.values_mut() {
        if game.is_custom() {
            let presence = game.executable.as_deref().map(savescummer_snapshots::presence).unwrap_or(Presence::Missing);
            match presence {
                Presence::Present => game.installed = true,
                Presence::Missing => game.installed = false,
                Presence::Unknown => {}
            }
            continue;
        }
        if !game.installed || seen.contains(&game.id) {
            continue;
        }
        let unsure = game.installs.iter().any(|install| {
            found.unreadable.iter().any(|lib| savescummer_core::common::is_within(&install.install_dir, lib, ci))
                || savescummer_snapshots::presence(&install.install_dir) == Presence::Unknown
        });
        if !unsure {
            game.installed = false;
        }
    }
    assign_install_tags(&mut inner);
    derive_all(host, &mut inner);
    let ids: Vec<String> = inner.games.keys().cloned().collect();
    for id in &ids {
        host.refresh_cache(&mut inner, id);
    }
    persist_games(host, &inner, counter);
    host.monitor_dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    let changes = install_changes(&inner, &before);
    drop(inner);
    for change in changes {
        crate::trace(&format!("{change} (scan: {reason})"));
    }
    let _ = full;
    new_games
}

/// Log lines for games whose installed state changed in this scan.
fn install_changes(inner: &Inner, before: &HashMap<String, bool>) -> Vec<String> {
    let mut out = Vec::new();
    for game in inner.games.values() {
        let was = before.get(&game.id).copied().unwrap_or(false);
        if game.installed == was {
            continue;
        }
        if game.installed {
            let place = match game.installs.first() {
                Some(install) => format!("{:?}, {}", install.store, install.install_dir.display()),
                None => game.executable.as_deref().map(|e| e.display().to_string()).unwrap_or_default(),
            };
            let again = if before.contains_key(&game.id) { " again" } else { "" };
            out.push(format!("installed{again}: {} ({}) at {place}", game.name, game.id));
        } else {
            out.push(format!("uninstalled: {} ({}); its history and checkpoints are kept", game.name, game.id));
        }
    }
    out.sort();
    out
}

/// Gives each of two installs of one game an install tag: the store's name,
/// or the install folder's name when both come from the same store.
fn assign_install_tags(inner: &mut Inner) {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for game in inner.games.values() {
        if let (Some(catalog), true) = (&game.catalog_id, game.installed) {
            groups.entry(catalog.clone()).or_default().push(game.id.clone());
        }
    }
    let mut tags: HashMap<String, Option<String>> = HashMap::new();
    for ids in groups.values() {
        if ids.len() < 2 {
            for id in ids {
                tags.insert(id.clone(), None);
            }
            continue;
        }
        for id in ids {
            let game = &inner.games[id];
            let store = game.installs.first().map(|i| i.store);
            let same_store =
                ids.iter().filter(|other| inner.games[*other].installs.first().map(|i| i.store) == store).count() > 1;
            let tag = if same_store {
                game.installs
                    .first()
                    .and_then(|i| i.install_dir.parent().and_then(|p| p.parent()).and_then(|p| p.parent()))
                    .and_then(|lib| lib.file_name())
                    .or_else(|| game.installs.first().and_then(|i| i.install_dir.file_name()))
                    .map(|n| n.to_string_lossy().into_owned())
            } else {
                store.map(|s| s.display_name().to_string())
            };
            tags.insert(id.clone(), tag);
        }
    }
    for game in inner.games.values_mut() {
        if !game.is_custom() {
            game.install_tag = tags.get(&game.id).cloned().flatten();
        }
    }
}

fn next_counter(host: &Host) -> u64 {
    db::setting(host.db().conn(), SETTING_COUNTER).ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(0)
}

pub fn persist_games(host: &Host, inner: &Inner, counter: u64) {
    let mut storage = host.db();
    let _ = storage.write(|c| {
        for game in inner.games.values() {
            db::put_game(c, &game.id, &serde_json::to_string(game).expect("game serializes"))?;
        }
        db::set_setting(c, SETTING_COUNTER, &counter.to_string())
    });
}

pub fn persist_game(host: &Host, game: &Game) -> Result<(), Failure> {
    let mut storage = host.db();
    storage
        .write(|c| db::put_game(c, &game.id, &serde_json::to_string(game).expect("game serializes")))
        .map_err(|e| Failure::new(ErrorKind::NotRecorded, e.to_string()))
}

/// Re-resolves one known game (a Steam game starting under another
/// account). Returns whether its save set changed.
pub fn reresolve(host: &Host, inner: &mut Inner, game_id: &str) -> bool {
    let Some(game) = inner.games.get(game_id) else { return false };
    let Some(catalog_id) = &game.catalog_id else { return false };
    let bundle = host.bundle();
    let Some(entry) = bundle.game(catalog_id) else { return false };
    let probe = RealProbe::new(&host.env);
    let installs: Vec<Install> = game.installs.clone();
    let decisions: Vec<Decision> = installs.iter().map(|i| resolve(entry, i, &probe)).collect();
    let mut known = HashMap::new();
    if let Some(Some(identity)) = game.identities.first() {
        known.insert(game_id.to_string(), identity.clone());
    }
    let records = assign_games(&decisions, &probe, &known);
    let Some(record) = records.first() else { return false };
    let game = inner.games.get_mut(game_id).expect("checked above");
    let changed = game.outcome.as_ref() != Some(&record.outcome) || game.context.as_ref() != Some(&record.context);
    game.outcome = Some(record.outcome.clone());
    game.context = Some(record.context.clone());
    game.catalog_executables = record.executables.clone();
    let snapshot = game.clone();
    if changed {
        let _ = persist_game(host, &snapshot);
        derive_one(host, inner, game_id);
        host.refresh_cache(inner, game_id);
    }
    changed
}

/// The current Steam account differs from the one a game was resolved in.
pub fn steam_account_changed(host: &Host, game: &Game) -> bool {
    if !game.is_steam() || game.location.is_some() {
        return false;
    }
    let current = RealProbe::new(&host.env).steam_account().map(|a| a.account_id);
    game.context.as_ref().is_some_and(|c| c.steam_account != current)
}

/// Validates and adds a custom game. Nothing partial is left behind.
pub fn add_custom(host: &Host, name: &str, executable: &str, location: &str) -> Result<String, Failure> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Failure::new(ErrorKind::InvalidConfig, "the name can't be blank"));
    }
    ask_for_typed(host, &[Some(executable), Some(location)])?;
    let exe = absolute(executable)?;
    let location = user_location(location)?;
    let mut inner = host.lock();
    let counter = next_counter(host) + 1;
    let id = crate::host::new_id("custom");
    let game = Game {
        id: id.clone(),
        kind: GameKind::Custom,
        catalog_id: None,
        name: name.to_string(),
        info: None,
        installed: savescummer_snapshots::presence(&exe) != Presence::Missing,
        installs: Vec::new(),
        identities: Vec::new(),
        catalog_executables: Vec::new(),
        outcome: None,
        context: None,
        warnings: Vec::new(),
        install_tag: None,
        executable: Some(exe),
        location: Some(location),
        created: counter,
    };
    validate_candidate(host, &mut inner, game.clone())?;
    {
        let mut storage = host.db();
        storage
            .write(|c| {
                db::put_game(c, &id, &serde_json::to_string(&game).expect("game serializes"))?;
                db::set_setting(c, SETTING_COUNTER, &counter.to_string())
            })
            .map_err(|e| Failure::new(ErrorKind::NotRecorded, e.to_string()))?;
    }
    inner.games.insert(id.clone(), game);
    derive_one(host, &mut inner, &id);
    host.refresh_cache(&mut inner, &id);
    host.monitor_dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    host.publish(&mut inner);
    drop(inner);
    // A save location on another drive is expected from now on.
    crate::scan::remember_drives(host);
    Ok(id)
}

/// Checks a changed game record as a whole: every new value must validate,
/// or the old configuration stays exactly as it was.
fn validate_candidate(host: &Host, inner: &mut Inner, candidate: Game) -> Result<(), Failure> {
    let id = candidate.id.clone();
    let previous = inner.games.insert(id.clone(), candidate);
    let previous_derived = inner.derived.get(&id).cloned();
    derive_one(host, inner, &id);
    let result = match &inner.derived[&id].active {
        Ok(_) => Ok(()),
        Err(f) if f.kind == ErrorKind::NoSaveLocation && inner.games[&id].location.is_none() => Ok(()),
        Err(f) => Err(f.clone()),
    };
    // Put the old record back; the caller commits the new one on success.
    match previous {
        Some(old) => {
            inner.games.insert(id.clone(), old);
        }
        None => {
            inner.games.remove(&id);
        }
    }
    match previous_derived {
        Some(d) => {
            inner.derived.insert(id, d);
        }
        None => {
            inner.derived.remove(&id);
        }
    }
    result
}

pub struct ConfigureRequest<'a> {
    pub name: Option<&'a str>,
    pub executable: Option<&'a str>,
    pub save_location: Option<&'a str>,
    pub reset_executable: bool,
    pub reset_save_location: bool,
}

/// Changes a game's configuration. Applied only after every new value
/// validates; changing the save set never moves or rewrites checkpoints.
pub fn configure(host: &Host, game_id: &str, request: ConfigureRequest<'_>) -> Result<(), Failure> {
    ask_for_typed(host, &[request.executable, request.save_location])?;
    let mut inner = host.lock();
    if inner.busy.contains_key(game_id) {
        return Err(Failure::new(ErrorKind::Busy, "another operation runs for this game").game(game_id));
    }
    let mut game = inner.game(game_id)?.clone();
    if let Some(name) = request.name {
        if !game.is_custom() {
            return Err(Failure::new(ErrorKind::InvalidRequest, "a known game's name comes from the catalog"));
        }
        if name.trim().is_empty() {
            return Err(Failure::new(ErrorKind::InvalidConfig, "the name can't be blank"));
        }
        game.name = name.trim().to_string();
    }
    if request.reset_executable {
        if game.is_custom() {
            return Err(Failure::new(ErrorKind::InvalidRequest, "a custom game has no catalog executable"));
        }
        game.executable = None;
    }
    if request.reset_save_location {
        if game.is_custom() {
            return Err(Failure::new(ErrorKind::InvalidRequest, "a custom game has no catalog save location"));
        }
        game.location = None;
    }
    if let Some(exe) = request.executable {
        game.executable = Some(absolute(exe)?);
        if game.is_custom() {
            game.installed = savescummer_snapshots::presence(game.executable.as_deref().unwrap()) != Presence::Missing;
        }
    }
    if let Some(location) = request.save_location {
        game.location = Some(user_location(location)?);
    }
    validate_candidate(host, &mut inner, game.clone())?;
    persist_game(host, &game)?;
    inner.games.insert(game_id.to_string(), game);
    derive_one(host, &mut inner, game_id);
    host.refresh_cache(&mut inner, game_id);
    host.bump_history(&mut inner, game_id);
    host.monitor_dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    host.publish(&mut inner);
    drop(inner);
    crate::scan::remember_drives(host);
    Ok(())
}

fn absolute(text: &str) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(text.trim());
    if !path.is_absolute() {
        return Err(Failure::new(ErrorKind::InvalidConfig, "use a full path").path(&path));
    }
    Ok(path)
}

/// Adding or configuring a game is a user action: a typed program or save
/// location macOS guards is asked for right away, while the user is still at
/// the form, and before the host's state is locked (the answer may take
/// long).
fn ask_for_typed(host: &Host, typed: &[Option<&str>]) -> Result<(), Failure> {
    for text in typed.iter().flatten() {
        let path = absolute(text)?;
        let root = split_location(&path).map_or(path, |target| target.root);
        crate::privacy::ask_for(host, &root)?;
    }
    Ok(())
}

/// A typed save location, validated.
fn user_location(text: &str) -> Result<UserLocation, Failure> {
    let path = absolute(text)?;
    let target =
        split_location(&path).ok_or_else(|| Failure::new(ErrorKind::InvalidConfig, "use a full path").path(&path))?;
    let real_root = savescummer_snapshots::real_path(&target.root).map_err(|e| {
        Failure::new(ErrorKind::InvalidTarget, format!("a link can't be resolved: {e}")).path(&target.root)
    })?;
    Ok(UserLocation { text: path.to_string_lossy().into_owned(), real_root })
}

/// Loads game records from the database.
pub fn load_games(host: &Host, inner: &mut Inner) {
    let rows = db::games(host.db().conn()).unwrap_or_default();
    for (id, data) in rows {
        if let Ok(game) = serde_json::from_str::<Game>(&data) {
            inner.games.insert(id, game);
        }
    }
}
