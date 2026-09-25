//! What a target matches on disk: its filter, minus its excludes and the
//! built-in excludes. Links and special files are never followed or copied;
//! meeting one fails the walk.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use savescummer_catalog::glob::{could_match_below, match_path};
use savescummer_core::{ErrorKind, Failure, Filter, builtin_excluded};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Root-relative path segments.
    pub rel: Vec<String>,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl Entry {
    pub fn rel_text(&self) -> String {
        self.rel.join("/")
    }
}

/// Everything a target matches, files and folders, in a stable order.
/// A missing root matches nothing.
pub fn walk_target(root: &Path, filter: &Filter, excludes: &[String], ci: bool) -> Result<Vec<Entry>, Failure> {
    if crate::fsx::is_guarded(root) {
        return Err(crate::fsx::guarded_failure(root));
    }
    let mut out = Vec::new();
    match fs::symlink_metadata(root) {
        Err(_) => return Ok(out),
        Ok(meta) if !meta.is_dir() => {
            return Err(Failure::new(ErrorKind::InvalidTarget, "the save location's root isn't a folder").path(root));
        }
        Ok(_) => {}
    }
    let walker = Walker { excludes, ci };
    match filter {
        Filter::All => walker.everything(root, &mut Vec::new(), &mut out)?,
        Filter::Exact(name) => {
            let path = root.join(name);
            match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Failure::new(ErrorKind::ReadFailed, e.to_string()).path(&path)),
                Ok(meta) => {
                    let mut rel = vec![name.clone()];
                    walker.visit(&path, &meta, &mut rel, &mut out, None)?;
                }
            }
        }
        Filter::Pattern(pattern) => walker.pattern(root, pattern, &mut Vec::new(), &mut out)?,
    }
    Ok(out)
}

struct Walker<'a> {
    excludes: &'a [String],
    ci: bool,
}

impl Walker<'_> {
    fn excluded(&self, rel: &[String], is_dir: bool) -> bool {
        let name = rel.last().map(String::as_str).unwrap_or("");
        if builtin_excluded(name, is_dir) {
            return true;
        }
        let text = rel.join("/");
        self.excludes.iter().any(|ex| match_path(ex, &text, self.ci))
    }

    /// Adds one entry that the filter matched, and everything inside it.
    fn visit(
        &self,
        path: &Path,
        meta: &fs::Metadata,
        rel: &mut Vec<String>,
        out: &mut Vec<Entry>,
        _pattern: Option<&str>,
    ) -> Result<(), Failure> {
        crate::fsx::advance();
        let file_type = meta.file_type();
        if self.excluded(rel, file_type.is_dir()) {
            return Ok(());
        }
        if file_type.is_symlink() {
            return Err(Failure::new(ErrorKind::LinkInTarget, "a link inside the save location").path(path));
        }
        if file_type.is_dir() {
            out.push(Entry { rel: rel.clone(), is_dir: true, size: 0, modified: meta.modified().ok() });
            self.everything(path, rel, out)
        } else if file_type.is_file() {
            out.push(Entry { rel: rel.clone(), is_dir: false, size: meta.len(), modified: meta.modified().ok() });
            Ok(())
        } else {
            Err(Failure::new(ErrorKind::LinkInTarget, "a special file inside the save location").path(path))
        }
    }

    fn children(&self, dir: &Path) -> Result<Vec<(String, std::path::PathBuf, fs::Metadata)>, Failure> {
        let read = fs::read_dir(dir).map_err(|e| read_failure(&e, dir))?;
        let mut children = Vec::new();
        for entry in read {
            let entry = entry.map_err(|e| read_failure(&e, dir))?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).map_err(|e| read_failure(&e, &path))?;
            children.push((entry.file_name().to_string_lossy().into_owned(), path, meta));
        }
        children.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(children)
    }

    fn everything(&self, dir: &Path, rel: &mut Vec<String>, out: &mut Vec<Entry>) -> Result<(), Failure> {
        for (name, path, meta) in self.children(dir)? {
            rel.push(name);
            self.visit(&path, &meta, rel, out, None)?;
            rel.pop();
        }
        Ok(())
    }

    fn pattern(&self, dir: &Path, pattern: &str, rel: &mut Vec<String>, out: &mut Vec<Entry>) -> Result<(), Failure> {
        for (name, path, meta) in self.children(dir)? {
            rel.push(name);
            let text = rel.join("/");
            if match_path(pattern, &text, self.ci) {
                self.visit(&path, &meta, rel, out, Some(pattern))?;
            } else if meta.is_dir()
                && !meta.file_type().is_symlink()
                && could_match_below(pattern, &text, self.ci)
                && !self.excluded(rel, true)
            {
                self.pattern(&path, pattern, rel, out)?;
            }
            rel.pop();
        }
        Ok(())
    }
}

fn read_failure(e: &std::io::Error, path: &Path) -> Failure {
    let kind = if crate::fsx::is_unavailable(e) { ErrorKind::TargetUnavailable } else { ErrorKind::ReadFailed };
    Failure::new(kind, e.to_string()).path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let path = dir.path().join(f);
            if f.ends_with('/') {
                fs::create_dir_all(&path).unwrap();
            } else {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, f.as_bytes()).unwrap();
            }
        }
        dir
    }

    fn rels(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| format!("{}{}", e.rel_text(), if e.is_dir { "/" } else { "" })).collect()
    }

    #[test]
    fn everything_minus_builtin_excludes() {
        let dir = tree(&[
            "a.sav",
            "Player.log",
            "logs/x.txt",
            "Crashes/dump.dmp",
            "steam_autocloud.vdf",
            "b.ssold",
            "sub/c.sav",
        ]);
        let entries = walk_target(dir.path(), &Filter::All, &[], true).unwrap();
        assert_eq!(rels(&entries), vec!["a.sav", "sub/", "sub/c.sav"]);
    }

    #[test]
    fn exact_name_file_or_folder() {
        let dir = tree(&["data/save_data.xml", "data/asset.bin", "saves/1.sav"]);
        let file = walk_target(&dir.path().join("data"), &Filter::Exact("save_data.xml".into()), &[], true).unwrap();
        assert_eq!(rels(&file), vec!["save_data.xml"]);
        let folder = walk_target(dir.path(), &Filter::Exact("saves".into()), &[], true).unwrap();
        assert_eq!(rels(&folder), vec!["saves/", "saves/1.sav"]);
        let missing = walk_target(dir.path(), &Filter::Exact("nope".into()), &[], true).unwrap();
        assert!(missing.is_empty());
    }

    #[test]
    fn patterns_and_excludes() {
        let dir = tree(&["user_0.dat", "user_1.dat", "dc_options.json", "C1/SGS1", "C1/other", "D1/SGS1"]);
        let p = walk_target(dir.path(), &Filter::Pattern("user_*.dat".into()), &[], true).unwrap();
        assert_eq!(rels(&p), vec!["user_0.dat", "user_1.dat"]);
        let deep = walk_target(dir.path(), &Filter::Pattern("C*/SGS*".into()), &[], true).unwrap();
        assert_eq!(rels(&deep), vec!["C1/SGS1"]);
        let ex = walk_target(dir.path(), &Filter::All, &["dc_options.json".into(), "C1/other".into()], true).unwrap();
        assert!(!rels(&ex).contains(&"dc_options.json".to_string()));
        assert!(!rels(&ex).contains(&"C1/other".to_string()));
    }

    #[test]
    fn a_missing_root_matches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(walk_target(&dir.path().join("x"), &Filter::All, &[], true).unwrap().is_empty());
    }
}
