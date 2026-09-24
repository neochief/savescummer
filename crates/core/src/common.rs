//! Checkpoints belong to their targets: a checkpoint is restored only into
//! targets it has in common with the game's current save set (same real
//! root, same filter). One rule for account switches, build switches,
//! overrides and catalog updates.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Filter;

/// A target as a checkpoint recorded it at Save time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedTarget {
    pub root: PathBuf,
    pub filter: Filter,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excludes: Vec<String>,
    /// The root didn't exist at Save time; a Load leaves it alone.
    #[serde(default)]
    pub absent: bool,
    /// The checkpoint subfolder holding what this target matched.
    pub folder: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Commonality {
    /// Every target the checkpoint holds is in the current save set.
    All,
    /// Some are: the checkpoint restores only those.
    Some,
    /// None: the checkpoint is unavailable until its targets return.
    None,
}

/// Pairs `(checkpoint target index, current target index)` in common.
pub fn common_pairs(
    recorded: &[RecordedTarget],
    current: &[(PathBuf, Filter)],
    case_insensitive: bool,
) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for (i, r) in recorded.iter().enumerate() {
        if let Some(j) = current.iter().position(|(root, filter)| {
            same_path(&r.root, root, case_insensitive) && r.filter.same(filter, case_insensitive)
        }) {
            pairs.push((i, j));
        }
    }
    pairs
}

/// How much of a checkpoint the current save set can take. Targets that were
/// absent at Save time hold nothing to restore, so they don't make a
/// checkpoint usable on their own.
pub fn commonality(recorded: &[RecordedTarget], current: &[(PathBuf, Filter)], case_insensitive: bool) -> Commonality {
    let pairs = common_pairs(recorded, current, case_insensitive);
    let with_data = pairs.iter().filter(|(i, _)| !recorded[*i].absent).count();
    if with_data == 0 {
        Commonality::None
    } else if pairs.len() == recorded.len() {
        Commonality::All
    } else {
        Commonality::Some
    }
}

/// Compares two real paths under the file system's case rules.
pub fn same_path(a: &Path, b: &Path, case_insensitive: bool) -> bool {
    path_key(a, case_insensitive) == path_key(b, case_insensitive)
}

/// A comparable spelling of a path: `/` separators, no trailing separator,
/// folded case where the file system ignores it.
pub fn path_key(path: &Path, case_insensitive: bool) -> String {
    let mut text = path.to_string_lossy().replace('\\', "/");
    while text.len() > 1 && text.ends_with('/') && !text.ends_with(":/") {
        text.pop();
    }
    if case_insensitive { text.to_lowercase() } else { text }
}

/// Whether `inner` is `outer` or inside it.
pub fn is_within(inner: &Path, outer: &Path, case_insensitive: bool) -> bool {
    let inner = path_key(inner, case_insensitive);
    let outer = path_key(outer, case_insensitive);
    if inner == outer {
        return true;
    }
    let prefix = if outer.ends_with('/') { outer } else { format!("{outer}/") };
    inner.starts_with(&prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(root: &str, name: &str, absent: bool) -> RecordedTarget {
        RecordedTarget {
            root: root.into(),
            filter: Filter::Exact(name.into()),
            excludes: vec![],
            absent,
            folder: name.into(),
        }
    }

    fn cur(root: &str, name: &str) -> (PathBuf, Filter) {
        (root.into(), Filter::Exact(name.into()))
    }

    #[test]
    fn all_some_none() {
        let recorded = [rec("C:/G", "a", false), rec("C:/G", "b", false)];
        assert_eq!(commonality(&recorded, &[cur("c:/g", "A"), cur("C:/G", "b")], true), Commonality::All);
        assert_eq!(commonality(&recorded, &[cur("C:/G", "a"), cur("C:/G", "c")], true), Commonality::Some);
        assert_eq!(commonality(&recorded, &[cur("C:/Other", "a")], true), Commonality::None);
    }

    #[test]
    fn case_rules_follow_the_file_system() {
        let recorded = [rec("/home/u/G", "a", false)];
        assert_eq!(commonality(&recorded, &[cur("/home/u/g", "a")], false), Commonality::None);
    }

    #[test]
    fn absent_targets_alone_make_nothing_usable() {
        let recorded = [rec("C:/G", "a", true), rec("C:/G", "b", false)];
        assert_eq!(commonality(&recorded, &[cur("C:/G", "a")], true), Commonality::None);
    }

    #[test]
    fn within() {
        assert!(is_within(Path::new("C:/a/b"), Path::new("c:/A"), true));
        assert!(!is_within(Path::new("C:/ab"), Path::new("C:/a"), true));
        assert!(is_within(Path::new("C:/a"), Path::new("C:/"), true));
    }
}
