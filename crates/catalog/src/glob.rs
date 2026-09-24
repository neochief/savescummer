//! Wildcard matching for target filters and excludes.
//!
//! One segment pattern supports `*`, `?` and `[...]` (with `!`/`^` negation
//! and ranges). A path pattern is `/`-separated segments, where a `**`
//! segment matches any number of segments, including none.

pub fn has_wildcard(segment: &str) -> bool {
    segment.contains(['*', '?', '['])
}

/// Matches one name against one segment pattern.
pub fn match_segment(pattern: &str, name: &str, case_insensitive: bool) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = if case_insensitive {
        (fold(pattern), fold(name))
    } else {
        (pattern.chars().collect(), name.chars().collect())
    };
    match_chars(&p, &n)
}

fn fold(text: &str) -> Vec<char> {
    text.chars().flat_map(char::to_lowercase).collect()
}

fn match_chars(p: &[char], n: &[char]) -> bool {
    // Iterative matcher with single-star backtracking.
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some((pi, ni));
                    pi += 1;
                    continue;
                }
                '?' => {
                    pi += 1;
                    ni += 1;
                    continue;
                }
                '[' => {
                    if let Some((matched, next)) = match_class(&p[pi..], n[ni]) {
                        if matched {
                            pi += next;
                            ni += 1;
                            continue;
                        }
                    } else if n[ni] == '[' {
                        pi += 1;
                        ni += 1;
                        continue;
                    }
                }
                c if c == n[ni] => {
                    pi += 1;
                    ni += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((sp, sn)) => {
                pi = sp + 1;
                ni = sn + 1;
                star = Some((sp, sn + 1));
            }
            None => return false,
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Matches a `[...]` class at the start of `p`. Returns whether `c` matched
/// and how many pattern chars the class used, or None if it isn't closed.
fn match_class(p: &[char], c: char) -> Option<(bool, usize)> {
    let mut i = 1;
    let negate = matches!(p.get(i), Some('!') | Some('^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < p.len() {
        if p[i] == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            if p[i] <= c && c <= p[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

/// Matches a relative `/`-separated path against a path pattern.
pub fn match_path(pattern: &str, path: &str, case_insensitive: bool) -> bool {
    let p: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let n: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match_segments(&p, &n, case_insensitive)
}

fn match_segments(p: &[&str], n: &[&str], ci: bool) -> bool {
    match p.first() {
        None => n.is_empty(),
        Some(&"**") => (0..=n.len()).any(|skip| match_segments(&p[1..], &n[skip..], ci)),
        Some(seg) => !n.is_empty() && match_segment(seg, n[0], ci) && match_segments(&p[1..], &n[1..], ci),
    }
}

/// Whether a relative path could still lead to a match deeper down: every
/// segment so far matches the pattern's leading segments. Used to prune a
/// walk without descending into folders a pattern can never reach.
pub fn could_match_below(pattern: &str, path: &str, case_insensitive: bool) -> bool {
    let p: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let n: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    prefix_match(&p, &n, case_insensitive)
}

fn prefix_match(p: &[&str], n: &[&str], ci: bool) -> bool {
    if n.is_empty() {
        return !p.is_empty();
    }
    match p.first() {
        None => false,
        Some(&"**") => true,
        Some(seg) => match_segment(seg, n[0], ci) && prefix_match(&p[1..], &n[1..], ci),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments() {
        assert!(match_segment("user_*.dat", "user_0.dat", false));
        assert!(!match_segment("user_*.dat", "user_0.json", false));
        assert!(match_segment("Slot?.save", "SlotA.save", false));
        assert!(match_segment("save*", "save", false));
        assert!(match_segment("[abc]x", "bx", false));
        assert!(!match_segment("[!abc]x", "bx", false));
        assert!(match_segment("[a-c]x", "cx", false));
        assert!(match_segment("*", "anything", false));
        assert!(match_segment("SAVE*", "save1", true));
        assert!(!match_segment("SAVE*", "save1", false));
        assert!(match_segment("a*b*c", "aXXbYYc", false));
        assert!(!match_segment("a*b*c", "aXXbYY", false));
    }

    #[test]
    fn paths() {
        assert!(match_path("C*/SGS*", "C1/SGS2", false));
        assert!(!match_path("C*/SGS*", "C1", false));
        assert!(match_path("**/x.sav", "a/b/x.sav", false));
        assert!(match_path("**/x.sav", "x.sav", false));
        assert!(could_match_below("C*/SGS*", "C1", false));
        assert!(!could_match_below("C*/SGS*", "D1", false));
        assert!(!could_match_below("C*/SGS*", "C1/SGS1", false));
    }
}
