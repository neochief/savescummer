use savescummer_core::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

mod identity;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub info: String,
    #[serde(default)]
    pub stores: Stores,
    pub platforms: BTreeMap<String, PlatformDefinition>,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stores {
    pub steam: Option<u32>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformDefinition {
    pub executables: Vec<String>,
    pub data_dir: String,
    #[serde(default)]
    pub known_install_dirs: Vec<String>,
    /// Exact uninstall subkey names, never fuzzy display-name matching.
    #[serde(default)]
    pub registry_keys: Vec<String>,
    #[serde(default)]
    pub alternative_data_dirs: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub game_id: String,
    pub name: String,
    pub info: String,
    pub install_dir: PathBuf,
    pub executables: Vec<PathBuf>,
    pub data_dir: PathBuf,
}
#[derive(Debug, Default, Serialize)]
pub struct Scan {
    pub candidates: Vec<Candidate>,
    pub errors: Vec<String>,
    pub library_roots: Vec<PathBuf>,
}
pub trait Discovery {
    fn steam_roots(&self) -> Vec<PathBuf>;
    fn known_folders(&self) -> BTreeMap<String, PathBuf>;
    fn platform(&self) -> &'static str;
    fn applications(&self) -> ApplicationScan {
        ApplicationScan::default()
    }
}
#[derive(Debug, Clone)]
pub struct ApplicationRecord {
    pub key: String,
    pub install_dir: PathBuf,
}
#[derive(Debug, Default)]
pub struct ApplicationScan {
    pub records: Vec<ApplicationRecord>,
    pub errors: Vec<String>,
}
pub fn parse_catalog(yaml: &str) -> Result<Definition> {
    let definition: Definition = serde_yaml::from_str(yaml)
        .map_err(|e| Error::new(ErrorCode::InvalidRequest, e.to_string()))?;
    if definition.id.is_empty() || definition.name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidRequest,
            "catalog ID and name must not be empty",
        ));
    }
    for platform in definition.platforms.values() {
        if platform.executables.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidRequest,
                "catalog must declare executable paths",
            ));
        }
        for exe in &platform.executables {
            validate_relative(exe)?;
        }
        let (_, relative) = platform
            .data_dir
            .split_once("}/")
            .ok_or_else(|| Error::new(ErrorCode::InvalidPath, "expected {ROOT}/relative/path"))?;
        validate_relative(relative)?;
        for template in platform
            .known_install_dirs
            .iter()
            .chain(&platform.alternative_data_dirs)
        {
            let (root, relative) = template
                .split_once("}/")
                .ok_or_else(|| invalid("expected {ROOT}/relative/path"))?;
            if !root.starts_with('{') {
                return Err(invalid("invalid path root"));
            }
            validate_relative(relative)?;
        }
        for key in &platform.registry_keys {
            if key.is_empty() || key.contains(['/', '\\', '\0']) {
                return Err(invalid(
                    "registry locators must be exact uninstall subkey names",
                ));
            }
        }
    }
    Ok(definition)
}
fn validate_relative(path: &str) -> Result<()> {
    if path.is_empty()
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path.contains(':')
        || path.split(['/', '\\']).any(|s| s == ".." || s.is_empty())
    {
        return Err(Error::new(
            ErrorCode::InvalidPath,
            "catalog paths must be nonempty relative paths without traversal",
        ));
    }
    Ok(())
}
pub fn scan(definitions: &[Definition], discovery: &dyn Discovery) -> Scan {
    let mut scan = Scan::default();
    let mut libraries = BTreeSet::new();
    for root in discovery.steam_roots() {
        libraries.insert(root.clone());
        let path = root.join("steamapps/libraryfolders.vdf");
        if path.exists() {
            match read_vdf(&path) {
                Ok(vdf) => {
                    if let Some(Value::Object(folders)) = get(&vdf, "libraryfolders") {
                        for (_, value) in folders {
                            if let Value::Object(folder) = value
                                && let Some(Value::Text(path)) = get(folder, "path")
                            {
                                libraries.insert(PathBuf::from(path));
                            }
                        }
                    }
                }
                Err(error) => scan.errors.push(error.message),
            }
        }
    }
    scan.library_roots = libraries.iter().cloned().collect();
    let folders = discovery.known_folders();
    let mut seen = BTreeSet::new();
    for library in libraries {
        for definition in definitions {
            let Some(app_id) = definition.stores.steam else {
                continue;
            };
            let Some(platform) = definition.platforms.get(discovery.platform()) else {
                continue;
            };
            let manifest = library.join(format!("steamapps/appmanifest_{app_id}.acf"));
            if !manifest.exists() {
                continue;
            }
            let attempt = (|| -> Result<_> {
                let vdf = read_vdf(&manifest)?;
                let Some(Value::Object(app)) = get(&vdf, "AppState") else {
                    return Err(invalid("missing AppState"));
                };
                if !matches!(get(app, "appid"), Some(Value::Text(id)) if id == &app_id.to_string())
                {
                    return Err(invalid("Steam app ID mismatch"));
                }
                let Some(Value::Text(relative)) = get(app, "installdir") else {
                    return Err(invalid("missing installdir"));
                };
                validate_relative(relative)?;
                let install_dir = library.join("steamapps/common").join(relative);
                let executables = platform
                    .executables
                    .iter()
                    .map(|e| install_dir.join(e))
                    .collect::<Vec<_>>();
                if !executables.iter().any(|p| p.is_file()) {
                    return Err(invalid(format!("{} executable not found", definition.id)));
                }
                let (root, relative) = platform
                    .data_dir
                    .split_once("}/")
                    .ok_or_else(|| invalid("invalid data path"))?;
                let root = root
                    .strip_prefix('{')
                    .ok_or_else(|| invalid("invalid data root"))?;
                let base = if root == "INSTALL_DIR" {
                    &install_dir
                } else {
                    folders
                        .get(root)
                        .ok_or_else(|| invalid(format!("unavailable known folder {root}")))?
                };
                let identity = identity::directory(&install_dir)
                    .map_err(|e| invalid(format!("{}: {e}", install_dir.display())))?;
                Ok((
                    Candidate {
                        game_id: definition.id.clone(),
                        name: definition.name.clone(),
                        info: definition.info.clone(),
                        data_dir: base.join(relative),
                        install_dir,
                        executables,
                    },
                    identity,
                ))
            })();
            match attempt {
                Ok((candidate, identity)) => {
                    if seen.insert((candidate.game_id.clone(), identity)) {
                        scan.candidates.push(candidate);
                    }
                }
                Err(error) => {
                    scan.errors
                        .push(format!("{}: {}", manifest.display(), error.message))
                }
            }
        }
    }
    let applications = discovery.applications();
    scan.errors.extend(applications.errors);
    for definition in definitions {
        let Some(platform) = definition.platforms.get(discovery.platform()) else {
            continue;
        };
        let mut installs = vec![];
        for template in &platform.known_install_dirs {
            match resolve_template(template, &folders, None) {
                Ok(path) => installs.push(path),
                Err(error) => scan.errors.push(error.message),
            }
        }
        if discovery.platform() == "windows" {
            let steam_key = definition.stores.steam.map(|id| format!("Steam App {id}"));
            for record in &applications.records {
                if steam_key
                    .as_ref()
                    .is_some_and(|key| key.eq_ignore_ascii_case(&record.key))
                    || platform
                        .registry_keys
                        .iter()
                        .any(|key| key.eq_ignore_ascii_case(&record.key))
                {
                    installs.push(record.install_dir.clone());
                }
            }
        }
        // Include Steam candidates so alternative data locations use the same path rules.
        installs.extend(
            scan.candidates
                .iter()
                .filter(|c| c.game_id == definition.id)
                .map(|c| c.install_dir.clone()),
        );
        for install in installs {
            if !install.is_absolute()
                || !platform
                    .executables
                    .iter()
                    .any(|exe| install.join(exe).is_file())
            {
                continue;
            }
            let identity = match identity::directory(&install) {
                Ok(identity) => identity,
                Err(error) => {
                    scan.errors.push(format!("{}: {error}", install.display()));
                    continue;
                }
            };
            for template in
                std::iter::once(&platform.data_dir).chain(&platform.alternative_data_dirs)
            {
                let data_dir = match resolve_template(template, &folders, Some(&install)) {
                    Ok(path) => path,
                    Err(error) => {
                        scan.errors.push(error.message);
                        continue;
                    }
                };
                // Identity deduplicates installation aliases; exact resolved data
                // locations remain separate choices, including not-yet-created DIRs.
                let duplicate = scan.candidates.iter().any(|candidate| {
                    let same_data = candidate.data_dir == data_dir
                        || matches!((identity::location(&candidate.data_dir), identity::location(&data_dir)), (Ok(a), Ok(b)) if a == b);
                    candidate.game_id == definition.id
                        && same_data
                        && identity::directory(&candidate.install_dir).ok().as_ref()
                            == Some(&identity)
                });
                if !duplicate {
                    scan.candidates.push(Candidate {
                        game_id: definition.id.clone(),
                        name: definition.name.clone(),
                        info: definition.info.clone(),
                        install_dir: install.clone(),
                        executables: platform
                            .executables
                            .iter()
                            .map(|exe| install.join(exe))
                            .collect(),
                        data_dir,
                    });
                }
            }
        }
    }
    scan
}

fn resolve_template(
    template: &str,
    folders: &BTreeMap<String, PathBuf>,
    install: Option<&Path>,
) -> Result<PathBuf> {
    let (root, relative) = template
        .split_once("}/")
        .ok_or_else(|| invalid("invalid path template"))?;
    let root = root
        .strip_prefix('{')
        .ok_or_else(|| invalid("invalid path root"))?;
    validate_relative(relative)?;
    let base = if root == "INSTALL_DIR" {
        install
    } else {
        folders.get(root).map(PathBuf::as_path)
    }
    .ok_or_else(|| invalid(format!("unavailable known folder {root}")))?;
    if !base.is_absolute() {
        return Err(invalid("path root must be absolute"));
    }
    Ok(base.join(relative))
}

#[derive(Debug)]
enum Value {
    Text(String),
    Object(Vec<(String, Value)>),
}
fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::InvalidRequest, message)
}
fn get<'a>(values: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    values
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value)
}
fn read_vdf(path: &Path) -> Result<Vec<(String, Value)>> {
    let text = fs::read_to_string(path).map_err(|e| invalid(format!("{}: {e}", path.display())))?;
    parse_vdf(&text)
}
fn parse_vdf(text: &str) -> Result<Vec<(String, Value)>> {
    // Valve's local metadata subset: quoted strings, braces and // comments.
    let mut tokens = vec![];
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => (),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '{' | '}' => tokens.push(c.to_string()),
            '"' => {
                let mut value = String::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == '"' {
                        closed = true;
                        break;
                    }
                    if c == '\\' {
                        match chars.peek() {
                            Some('"' | '\\') => value.push(chars.next().unwrap()),
                            _ => value.push(c),
                        }
                    } else {
                        value.push(c);
                    }
                }
                if !closed {
                    return Err(invalid("unterminated VDF string"));
                }
                tokens.push(format!("s{value}"));
            }
            _ => return Err(invalid("unsupported or malformed VDF token")),
        }
    }
    fn object(
        tokens: &[String],
        index: &mut usize,
        nested: bool,
        depth: usize,
    ) -> Result<Vec<(String, Value)>> {
        if depth > 32 {
            return Err(invalid("VDF nesting limit exceeded"));
        }
        let mut values = vec![];
        while let Some(token) = tokens.get(*index) {
            if token == "}" && nested {
                *index += 1;
                return Ok(values);
            }
            let key = token
                .strip_prefix('s')
                .ok_or_else(|| invalid("expected VDF key"))?
                .to_string();
            *index += 1;
            let token = tokens
                .get(*index)
                .ok_or_else(|| invalid("missing VDF value"))?;
            *index += 1;
            let value = if token == "{" {
                Value::Object(object(tokens, index, true, depth + 1)?)
            } else {
                Value::Text(
                    token
                        .strip_prefix('s')
                        .ok_or_else(|| invalid("expected VDF value"))?
                        .to_string(),
                )
            };
            values.push((key, value));
        }
        if nested {
            Err(invalid("unclosed VDF object"))
        } else {
            Ok(values)
        }
    }
    object(&tokens, &mut 0, false, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_rejects_path_traversal() {
        let catalog = include_str!("../../../catalog/games/void-war.yaml");
        assert!(parse_catalog(catalog).is_ok());
        assert!(parse_catalog(&catalog.replace("Void War.exe", "../other.exe")).is_err());
    }
    #[test]
    fn malformed_vdf_is_an_error() {
        for s in ["\"a\" {", "\"a\"", "\"unclosed", "}"] {
            assert!(parse_vdf(s).is_err());
        }
    }
}
