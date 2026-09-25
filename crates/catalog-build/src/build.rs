//! The build pipeline (PLAN-CATALOG.md Section 3): games.csv rows → manifest
//! lookup → translation → addendum merge → identity → validated bundle.

use std::collections::{BTreeMap, BTreeSet};

use savescummer_catalog::model::{leading_placeholder, placeholder_applies};
use savescummer_catalog::{Bundle, Game, PathRule, Platform, SCHEMA, Source};

use crate::inputs::{AddendumEntry, Fit, GameRow, Lock, parse_addendum, parse_games_csv};
use crate::manifest::{self, Manifest};
use crate::report::{FolderItem, Issue, IssueKind, MultiTargetItem, OverrideItem, Report};
use crate::translate::{Entry, Translation, normalize, translate};

/// The builder's inputs, as bytes and text already read from disk.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub games_csv: &'a str,
    pub addendum: &'a str,
    pub manifest: &'a [u8],
    pub lock: &'a str,
}

/// The result of a build: the bundle when there were no hard errors, and the
/// report either way.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub bundle: Option<Bundle>,
    pub report: Report,
}

/// Build switches. The default is the strict reading of Section 3.5.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Turns the two per-game hard errors (a `Keep` name that neither the
    /// manifest nor the addendum has, and a `Keep` game with no usable save
    /// target) into warnings and leaves those games out of the bundle. For
    /// when the manifest genuinely has nothing usable and nobody has checked
    /// the game on a real machine yet; every other hard error stays one.
    pub allow_unbuildable: bool,
}

/// Runs the whole build with default [`Options`].
pub fn build(inputs: &Inputs) -> Outcome {
    build_with(inputs, Options::default())
}

/// Runs the whole build. Never panics on bad input; every problem ends up in
/// the report, and any hard error means no bundle.
pub fn build_with(inputs: &Inputs, options: Options) -> Outcome {
    let mut report = Report::default();
    let fail = |mut report: Report, kind, message: String| {
        report.errors.push(Issue::new(kind, None, message));
        Outcome { bundle: None, report }
    };

    let lock = match Lock::parse(inputs.lock) {
        Ok(lock) => lock,
        Err(e) => return fail(report, IssueKind::InvalidInput, e),
    };
    let actual = manifest::sha256_hex(inputs.manifest);
    if !actual.eq_ignore_ascii_case(lock.sha256.trim()) {
        return fail(
            report,
            IssueKind::ManifestHash,
            format!("manifest sha256 is {actual}, but manifest.lock pins {}", lock.sha256),
        );
    }
    let manifest = match manifest::parse(inputs.manifest) {
        Ok(manifest) => manifest,
        Err(e) => return fail(report, IssueKind::InvalidInput, e),
    };
    let (rows, row_errors) = match parse_games_csv(inputs.games_csv) {
        Ok(parsed) => parsed,
        Err(e) => return fail(report, IssueKind::InvalidInput, e),
    };
    let addendum = match parse_addendum(inputs.addendum) {
        Ok(addendum) => addendum,
        Err(e) => return fail(report, IssueKind::AddendumSchema, e),
    };

    for error in row_errors {
        let kind =
            if error.message.contains("Product fit") { IssueKind::InvalidProductFit } else { IssueKind::InvalidInput };
        report.errors.push(Issue::new(
            kind,
            error.name.as_deref(),
            format!("games.csv line {}: {}", error.line, error.message),
        ));
    }

    // Duplicate names (case-insensitive): an error, and only the first row
    // is built so the rest of the report stays meaningful.
    let mut seen: BTreeMap<String, &GameRow> = BTreeMap::new();
    let mut unique: Vec<&GameRow> = Vec::new();
    for row in &rows {
        match seen.get(&row.name.to_lowercase()) {
            Some(first) => report.errors.push(Issue::new(
                IssueKind::DuplicateName,
                Some(&row.name),
                format!("games.csv lines {} and {} name the same game", first.line, row.line),
            )),
            None => {
                seen.insert(row.name.to_lowercase(), row);
                unique.push(row);
            }
        }
    }

    report.stats.keep_rows = unique.iter().filter(|r| r.fit == Fit::Keep).count();
    report.stats.remove_rows = unique.iter().filter(|r| r.fit == Fit::Remove).count();
    report.stats.ignored_rows = unique.iter().filter(|r| r.fit == Fit::Ignored).count();
    for row in unique.iter().filter(|r| r.fit == Fit::Ignored) {
        report.warnings.push(Issue::new(
            IssueKind::IgnoredGame,
            Some(&row.name),
            "marked `Ignored` in games.csv: not built until it's addressed",
        ));
    }

    let keep: BTreeSet<&str> = unique.iter().filter(|r| r.fit == Fit::Keep).map(|r| r.name.as_str()).collect();
    for name in addendum.keys() {
        if !keep.contains(name.as_str()) {
            report.warnings.push(Issue::new(
                IssueKind::AddendumWithoutKeepRow,
                Some(name),
                "addendum entry has no `Keep` row in games.csv; ignored",
            ));
        }
    }

    let mut games = Vec::new();
    for row in unique.iter().filter(|r| r.fit == Fit::Keep) {
        if let Some(game) = build_game(row, &manifest, addendum.get(&row.name), options, &mut report) {
            games.push(game);
        }
    }

    games.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.name.cmp(&b.name)));
    report.stats.games_built = games.len();
    report.review.multi_target.sort_by(|a, b| a.game.cmp(&b.game));

    let bundle = Bundle { schema: SCHEMA, source: Source { repo: lock.repo, revision: lock.revision }, games };
    if let Err(e) = bundle.validate() {
        report.errors.push(Issue::new(IssueKind::InvalidEntry, None, e.to_string()));
    }
    let bundle = if report.has_errors() { None } else { Some(bundle) };
    Outcome { bundle, report }
}

/// Builds one `Keep` game, or records why it can't be built.
fn build_game(
    row: &GameRow,
    manifest: &Manifest,
    addendum: Option<&AddendumEntry>,
    options: Options,
    report: &mut Report,
) -> Option<Game> {
    let name = row.name.as_str();
    // Where a per-game "can't be built" goes: an error, or with
    // `allow_unbuildable` a warning saying the game was left out.
    let unbuildable = |report: &mut Report, issue: Issue| {
        if options.allow_unbuildable {
            let message = format!("{} (left out: --allow-unbuildable)", issue.message);
            report.warnings.push(Issue { message, ..issue });
        } else {
            report.errors.push(issue);
        }
    };
    let translation: Option<Translation> = manifest.get(name).map(|game| translate(name, game));

    let mut entry = match (&translation, addendum) {
        (None, None) => {
            let suggestions = closest_names(name, manifest.keys().map(String::as_str), 3);
            let hint = if suggestions.is_empty() {
                String::new()
            } else {
                format!(
                    "; closest manifest names: {}",
                    suggestions.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>().join(", ")
                )
            };
            unbuildable(
                report,
                Issue::new(
                    IssueKind::UnresolvedName,
                    Some(name),
                    format!("not in the manifest and no addendum entry{hint}"),
                ),
            );
            return None;
        }
        (Some(t), None) => t.entry.clone(),
        (translation, Some(add)) => {
            let merged = merge(translation.as_ref().map(|t| &t.entry), add);
            if !merged.shadowed.is_empty() {
                report.warnings.push(Issue::new(
                    IssueKind::AddendumShadowed,
                    Some(name),
                    format!(
                        "addendum {} shadowed by the manifest and unused; delete {}",
                        merged.shadowed.join(", "),
                        if merged.shadowed.len() == 1 { "it" } else { "them" }
                    ),
                ));
            }
            if !add.overrides.is_empty() {
                report.review.overrides.push(OverrideItem {
                    game: name.to_string(),
                    fields: add.overrides.names().into_iter().map(str::to_string).collect(),
                });
            }
            merged.entry
        }
    };

    drop_foreign_placeholders(name, "save", &mut entry.save, report);
    drop_foreign_placeholders(name, "exclude", &mut entry.exclude, report);

    if let Some(t) = &translation {
        // A dropped target only matters while the manifest's `save` is used;
        // an `override` replacing it has already dealt with the game.
        let save_overridden = addendum.is_some_and(|a| a.overrides.save.is_some());
        if !save_overridden {
            for dropped in t.dropped.iter().filter(|d| d.reason.warns()) {
                report.warnings.push(Issue::new(
                    IssueKind::DroppedTarget,
                    Some(name),
                    format!("dropped {} ({}): {}", dropped.manifest_path, dropped.reason, dropped.detail),
                ));
            }
        }
        for rule in &entry.save {
            let listed = report.review.config_save_folders.iter().any(|f| f.game == name && f.path == rule.path);
            if t.config_save_folders.contains(&rule.path) && !listed {
                report.review.config_save_folders.push(FolderItem { game: name.to_string(), path: rule.path.clone() });
            }
        }
        for (path, why) in &t.dropped_launch {
            report.review.dropped_launch.push(FolderItem { game: name.to_string(), path: format!("{path} ({why})") });
        }
    }

    if entry.save.is_empty() {
        let reason = match &translation {
            Some(t) if !t.failure.is_empty() => t.failure.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", "),
            Some(_) => "the addendum removed every target".to_string(),
            None => "addendum entry has no `save`".to_string(),
        };
        unbuildable(
            report,
            Issue::new(IssueKind::NoUsableSave, Some(name), format!("no usable save target ({reason})")),
        );
        return None;
    }

    if translation.is_some() {
        report.stats.from_manifest += 1;
    } else {
        report.stats.from_addendum_only += 1;
    }

    let id = identity(&entry, addendum.and_then(|a| a.id.as_deref()), name);
    if entry.save.len() > 1 {
        report.review.multi_target.push(MultiTargetItem { game: name.to_string(), targets: entry.save.len() });
    }
    let game = Game {
        id,
        name: name.to_string(),
        info: row.info.clone(),
        detect: entry.detect,
        install_dirs: entry.install_dirs,
        executables: entry.executables,
        save: entry.save,
        exclude: entry.exclude,
    };
    if let Err(problem) = game.validate() {
        report.errors.push(Issue::new(IssueKind::InvalidEntry, Some(name), problem));
        return None;
    }
    Some(game)
}

/// Drops the rules for one OS whose builds never resolve the rule's leading
/// placeholder (PLAN-CATALOG.md 4.4), such as `{XDG_DATA_HOME}` for macOS,
/// with a warning each: the addendum gives that OS its real location.
fn drop_foreign_placeholders(name: &str, field: &str, rules: &mut Vec<PathRule>, report: &mut Report) {
    rules.retain(|rule| {
        let (Some(os), Some(placeholder)) = (rule.when.os, leading_placeholder(&rule.path)) else { return true };
        if placeholder_applies(placeholder, os) {
            return true;
        }
        report.warnings.push(Issue::new(
            IssueKind::DroppedTarget,
            Some(name),
            format!("dropped {field} {} (os: {os}): {{{placeholder}}} doesn't resolve on {os}", rule.path),
        ));
        false
    });
}

/// The merged entry plus the addendum parts the manifest made unused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Merged {
    pub entry: Entry,
    pub shadowed: Vec<String>,
}

/// Merges an addendum entry over a translated manifest entry (3.2). With no
/// manifest entry the addendum is the whole entry. `override` fields replace
/// the result's field last.
pub fn merge(manifest: Option<&Entry>, addendum: &AddendumEntry) -> Merged {
    let add = &addendum.fields;
    let mut shadowed = Vec::new();
    let mut entry = match manifest {
        None => Entry {
            detect: add.detect.clone().unwrap_or_default(),
            install_dirs: add.install_dirs.clone().unwrap_or_default(),
            executables: add.executables.clone().unwrap_or_default(),
            save: add.save.clone().unwrap_or_default(),
            exclude: add.exclude.clone().unwrap_or_default(),
        },
        Some(m) => {
            let mut entry = m.clone();
            // detect: per store key, manifest wins.
            if let Some(detect) = &add.detect {
                fill(&mut entry.detect.steam, &detect.steam, "detect.steam", &mut shadowed);
                fill(&mut entry.detect.gog, &detect.gog, "detect.gog", &mut shadowed);
                fill(&mut entry.detect.uninstall, &detect.uninstall, "detect.uninstall", &mut shadowed);
            }
            if let Some(dirs) = &add.install_dirs {
                fill(&mut entry.install_dirs, dirs, "installDirs", &mut shadowed);
            }
            // executables: per OS, manifest wins when it has a list.
            if let Some(exes) = &add.executables {
                for platform in Platform::ALL {
                    fill(
                        entry.executables.for_platform_mut(platform),
                        exes.for_platform(platform),
                        &format!("executables.{platform}"),
                        &mut shadowed,
                    );
                }
            }
            // save/exclude: per OS, the addendum keeps only the OSes the
            // manifest produced nothing for.
            if let Some(save) = &add.save {
                entry.save = merge_rules(&m.save, save, "save", &mut shadowed);
            }
            if let Some(exclude) = &add.exclude {
                entry.exclude = merge_rules(&m.exclude, exclude, "exclude", &mut shadowed);
            }
            entry
        }
    };

    if addendum.id.is_some() && !entry.detect.is_empty() && manifest.is_some() {
        shadowed.push("id".into());
    }

    // override: replaces outright, whatever the manifest or the addendum's
    // plain fields said.
    let o = &addendum.overrides;
    let mut replaced = |name: &str, plain_present: bool| {
        if plain_present {
            shadowed.push(format!("{name} (replaced by its own override)"));
        }
    };
    if let Some(detect) = &o.detect {
        entry.detect = detect.clone();
        replaced("detect", add.detect.is_some());
    }
    if let Some(dirs) = &o.install_dirs {
        entry.install_dirs = dirs.clone();
        replaced("installDirs", add.install_dirs.is_some());
    }
    if let Some(exes) = &o.executables {
        entry.executables = exes.clone();
        replaced("executables", add.executables.is_some());
    }
    if let Some(save) = &o.save {
        entry.save = save.clone();
        replaced("save", add.save.is_some());
    }
    if let Some(exclude) = &o.exclude {
        entry.exclude = exclude.clone();
        replaced("exclude", add.exclude.is_some());
    }
    Merged { entry, shadowed }
}

/// Manifest value wins when non-empty; otherwise the addendum's fills it.
fn fill<T: Clone>(target: &mut Vec<T>, addendum: &[T], name: &str, shadowed: &mut Vec<String>) {
    if addendum.is_empty() {
        return;
    }
    if target.is_empty() {
        *target = addendum.to_vec();
    } else {
        shadowed.push(name.to_string());
    }
}

/// The OSes a rule applies to (no `os` means all of them).
fn rule_platforms(rule: &PathRule) -> BTreeSet<Platform> {
    match rule.when.os {
        Some(os) => BTreeSet::from([os]),
        None => Platform::ALL.into_iter().collect(),
    }
}

fn merge_rules(manifest: &[PathRule], addendum: &[PathRule], name: &str, shadowed: &mut Vec<String>) -> Vec<PathRule> {
    let covered: BTreeSet<Platform> = manifest.iter().flat_map(rule_platforms).collect();
    let mut out = manifest.to_vec();
    let mut lost: BTreeSet<Platform> = BTreeSet::new();
    for rule in addendum {
        let platforms = rule_platforms(rule);
        if platforms.is_disjoint(&covered) {
            out.push(rule.clone());
        } else {
            lost.extend(platforms.intersection(&covered));
        }
    }
    if !lost.is_empty() {
        let oses: Vec<&str> = lost.iter().map(|p| p.as_str()).collect();
        shadowed.push(format!("{name} ({})", oses.join(", ")));
    }
    normalize(out)
}

/// The game's catalog id (3.3).
pub fn identity(entry: &Entry, addendum_id: Option<&str>, name: &str) -> String {
    if let Some(steam) = entry.detect.steam.first() {
        format!("steam-{steam}")
    } else if let Some(gog) = entry.detect.gog.first() {
        format!("gog-{gog}")
    } else if let Some(id) = addendum_id.map(str::trim).filter(|id| !id.is_empty()) {
        id.to_string()
    } else {
        slug(name)
    }
}

/// Lowercase letters and digits, every other run of characters one `-`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// The manifest names closest to `name`, best first. Names are compared
/// case- and punctuation-insensitively, so `Slay The Spire` finds
/// `Slay the Spire` at distance 0. A shared beginning counts in a name's
/// favor (series and subtitles differ at the end), so `Six Ages 2: …` finds
/// `Six Ages: …` before an unrelated name of similar length.
pub fn closest_names<'a>(name: &str, candidates: impl Iterator<Item = &'a str>, limit: usize) -> Vec<String> {
    let wanted: Vec<char> = fold(name);
    let mut scored: Vec<(usize, &str)> = candidates
        .map(|candidate| {
            let folded = fold(candidate);
            let prefix = wanted.iter().zip(&folded).take_while(|(a, b)| a == b).count();
            (levenshtein(&wanted, &folded).saturating_sub(prefix), candidate)
        })
        .collect();
    scored.sort();
    scored.into_iter().take(limit).map(|(_, n)| n.to_string()).collect()
}

fn fold(name: &str) -> Vec<char> {
    name.chars().flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric()).collect()
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != cb);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}
