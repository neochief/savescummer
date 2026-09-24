//! Identity derivation (Section 3.3): store-based ids are stable across upstream
//! renames, so history, overrides and checkpoints survive catalog updates.

use savescummer_catalog::model::Detect;

/// `steam-<id>`, else `gog-<id>`, else the addendum id, else a slug of the name.
pub fn derive(name: &str, detect: &Detect, addendum_id: Option<&str>) -> String {
    if let Some(id) = detect.steam.as_ref().and_then(|ids| ids.first()) {
        return format!("steam-{id}");
    }
    if let Some(id) = detect.gog.as_ref().and_then(|ids| ids.first()) {
        return format!("gog-{id}");
    }
    if let Some(id) = addendum_id.map(str::trim).filter(|id| !id.is_empty()) {
        return id.to_string();
    }
    slug(name)
}

pub fn slug(name: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            dash = false;
        } else if !dash && !slug.is_empty() {
            slug.push('-');
            dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "game".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use savescummer_catalog::model::IdList;

    #[test]
    fn prefers_steam_then_gog_then_addendum_then_slug() {
        let mut detect = Detect::default();
        assert_eq!(derive("Some Game!", &detect, None), "some-game");
        assert_eq!(derive("Some Game!", &detect, Some("custom")), "custom");
        detect.gog = Some(IdList::one(123));
        assert_eq!(derive("Some Game!", &detect, Some("custom")), "gog-123");
        detect.steam = Some(IdList::one(456));
        assert_eq!(derive("Some Game!", &detect, Some("custom")), "steam-456");
    }
}
