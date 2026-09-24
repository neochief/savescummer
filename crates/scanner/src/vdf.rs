//! A small reader for Valve's text KeyValues files (`libraryfolders.vdf`,
//! `appmanifest_*.acf`, `loginusers.vdf`, `registry.vdf`).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Map(Map),
}

/// Keys keep their file order; lookups ignore case like Steam does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Map {
    pub entries: Vec<(String, Value)>,
}

impl Map {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Text(t) => Some(t),
            Value::Map(_) => None,
        }
    }

    pub fn map(&self, key: &str) -> Option<&Map> {
        match self.get(key)? {
            Value::Map(m) => Some(m),
            Value::Text(_) => None,
        }
    }

    /// Follows a path of nested keys.
    pub fn path(&self, keys: &[&str]) -> Option<&Map> {
        keys.iter().try_fold(self, |map, key| map.map(key))
    }

    pub fn maps(&self) -> impl Iterator<Item = (&str, &Map)> {
        self.entries.iter().filter_map(|(k, v)| match v {
            Value::Map(m) => Some((k.as_str(), m)),
            Value::Text(_) => None,
        })
    }
}

pub fn parse(text: &str) -> Option<Map> {
    let tokens = tokenize(text)?;
    let mut pos = 0;
    let map = parse_map(&tokens, &mut pos, false)?;
    Some(map)
}

#[derive(Debug, PartialEq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '"' => {
                let mut s = String::new();
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            other => s.push(other),
                        },
                        other => s.push(other),
                    }
                }
                tokens.push(Token::Str(s));
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            _ => {
                // Unquoted token (rare in these files).
                let mut s = String::from(c);
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                tokens.push(Token::Str(s));
            }
        }
    }
    Some(tokens)
}

fn parse_map(tokens: &[Token], pos: &mut usize, nested: bool) -> Option<Map> {
    let mut map = Map::default();
    while *pos < tokens.len() {
        match &tokens[*pos] {
            Token::Close => {
                *pos += 1;
                return nested.then_some(map);
            }
            Token::Str(key) => {
                *pos += 1;
                match tokens.get(*pos)? {
                    Token::Str(value) => {
                        map.entries.push((key.clone(), Value::Text(value.clone())));
                        *pos += 1;
                    }
                    Token::Open => {
                        *pos += 1;
                        let inner = parse_map(tokens, pos, true)?;
                        map.entries.push((key.clone(), Value::Map(inner)));
                    }
                    Token::Close => return None,
                }
            }
            Token::Open => return None,
        }
    }
    (!nested).then_some(map)
}

/// A flat view used by tests and diagnostics.
pub fn flatten(map: &Map) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    fn walk(map: &Map, prefix: &str, out: &mut BTreeMap<String, String>) {
        for (k, v) in &map.entries {
            let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}/{k}") };
            match v {
                Value::Text(t) => {
                    out.insert(key, t.clone());
                }
                Value::Map(m) => walk(m, &key, out),
            }
        }
    }
    walk(map, "", &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_library_folders() {
        let text = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps"
		{
			"2200"		"513401161"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}"#;
        let map = parse(text).unwrap();
        let folders = map.map("LibraryFolders").unwrap();
        let paths: Vec<&str> = folders.maps().filter_map(|(_, m)| m.text("path")).collect();
        assert_eq!(paths, vec![r"C:\Program Files (x86)\Steam", r"D:\SteamLibrary"]);
    }

    #[test]
    fn rejects_broken_files() {
        assert!(parse(r#""a" { "b" "c" "#).is_none());
        assert!(parse(r#""a" }"#).is_none());
    }
}
