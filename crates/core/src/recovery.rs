//! The four recovery rules for interrupted operations (PLAN-HOST.md,
//! Recovery at startup). The host observes the files; these functions decide.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rule {
    /// Nothing live changed: delete our temporary material, mark failed.
    R1,
    /// Interrupted in stage 2 or 3 and every file is verifiably where the
    /// journal says: reverse the renames.
    R2,
    /// The result is verifiably in place but wasn't recorded: finish it.
    R3,
    /// Anything else: leave every file as it is and keep everything.
    R4,
}

/// Where a live file that a Load replaces or deletes is now, judged by its
/// recorded file identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Original {
    /// Still under its real name: not set aside yet.
    AtLive,
    /// Set aside under `.ssold`.
    AtOld,
    /// Gone: stage 4 deleted it.
    Gone,
    /// Somewhere we can't verify.
    Unknown,
}

/// Where a checkpoint file's copy is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Copy {
    /// Under `.ssnew`: copied in, not swapped yet.
    AtNew,
    /// Under the real name: swapped in.
    AtLive,
    /// Not there: stage 1 hadn't reached it.
    Missing,
    /// Something we can't verify.
    Unknown,
}

/// One file a Load covers. `original` is None for a file the Load adds;
/// `copy` is None for a file the Load deletes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub original: Option<Original>,
    pub copy: Option<Copy>,
}

/// Decides how to resolve an interrupted Load or Revert from where each file
/// is now.
pub fn classify_load(entries: &[Entry]) -> Rule {
    let unknown =
        entries.iter().any(|e| matches!(e.original, Some(Original::Unknown)) || matches!(e.copy, Some(Copy::Unknown)));
    if unknown {
        return Rule::R4;
    }
    let nothing_set_aside = entries.iter().all(|e| matches!(e.original, None | Some(Original::AtLive)));
    let nothing_swapped = entries.iter().all(|e| !matches!(e.copy, Some(Copy::AtLive)));
    if nothing_set_aside && nothing_swapped {
        return Rule::R1;
    }
    // Every copy is in place and every replaced file is set aside or gone.
    let all_swapped = entries.iter().all(|e| matches!(e.copy, None | Some(Copy::AtLive)))
        && entries.iter().all(|e| matches!(e.original, None | Some(Original::AtOld) | Some(Original::Gone)));
    if all_swapped {
        return Rule::R3;
    }
    // Stage 2 or 3 part-way: reversible only if nothing was deleted yet and
    // every copy that isn't swapped in still exists to swap back.
    let reversible = entries.iter().all(|e| {
        let original_ok = matches!(e.original, None | Some(Original::AtLive) | Some(Original::AtOld));
        let copy_ok = matches!(e.copy, None | Some(Copy::AtNew) | Some(Copy::AtLive));
        // A copy can only be swapped in once its original is out of the way.
        let order_ok = !(matches!(e.copy, Some(Copy::AtLive)) && matches!(e.original, Some(Original::AtLive)));
        original_ok && copy_ok && order_ok
    });
    if reversible { Rule::R2 } else { Rule::R4 }
}

/// Decides how to resolve an interrupted Save (or recovery checkpoint copy).
pub fn classify_save(temporary_exists: bool, published_matches: Option<bool>) -> Rule {
    match (temporary_exists, published_matches) {
        // Published under its checkpoint name and it's the folder we wrote.
        (_, Some(true)) => Rule::R3,
        // Something else sits under the name: don't touch it.
        (_, Some(false)) => Rule::R4,
        // Still copying (or the copy already vanished): nothing live changed.
        (_, None) => Rule::R1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(original: Option<Original>, copy: Option<Copy>) -> Entry {
        Entry { original, copy }
    }

    #[test]
    fn stage_one_is_rule_one() {
        let entries = [
            e(Some(Original::AtLive), Some(Copy::AtNew)),
            e(Some(Original::AtLive), Some(Copy::Missing)),
            e(None, Some(Copy::AtNew)),
        ];
        assert_eq!(classify_load(&entries), Rule::R1);
    }

    #[test]
    fn part_way_stage_two_is_rule_two() {
        let entries = [
            e(Some(Original::AtOld), Some(Copy::AtNew)),
            e(Some(Original::AtLive), Some(Copy::AtNew)),
            e(Some(Original::AtOld), None),
        ];
        assert_eq!(classify_load(&entries), Rule::R2);
    }

    #[test]
    fn part_way_stage_three_is_rule_two() {
        let entries = [
            e(Some(Original::AtOld), Some(Copy::AtLive)),
            e(Some(Original::AtOld), Some(Copy::AtNew)),
            e(None, Some(Copy::AtNew)),
        ];
        assert_eq!(classify_load(&entries), Rule::R2);
    }

    #[test]
    fn everything_swapped_is_rule_three() {
        let entries = [
            e(Some(Original::AtOld), Some(Copy::AtLive)),
            e(Some(Original::Gone), Some(Copy::AtLive)),
            e(Some(Original::AtOld), None),
            e(None, Some(Copy::AtLive)),
        ];
        assert_eq!(classify_load(&entries), Rule::R3);
    }

    #[test]
    fn anything_unverifiable_is_rule_four() {
        assert_eq!(classify_load(&[e(Some(Original::Unknown), Some(Copy::AtNew))]), Rule::R4);
        assert_eq!(classify_load(&[e(Some(Original::AtOld), Some(Copy::Unknown))]), Rule::R4);
        // A copy went missing after its original was set aside.
        assert_eq!(classify_load(&[e(Some(Original::AtOld), Some(Copy::Missing))]), Rule::R4);
        // A set-aside file was deleted while copies are still unswapped.
        assert_eq!(classify_load(&[e(Some(Original::Gone), Some(Copy::AtNew))]), Rule::R4);
    }

    #[test]
    fn saves() {
        assert_eq!(classify_save(true, None), Rule::R1);
        assert_eq!(classify_save(false, Some(true)), Rule::R3);
        assert_eq!(classify_save(false, Some(false)), Rule::R4);
    }
}
