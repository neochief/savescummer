//! Game records with the user's folders written portably (see
//! [`savescummer_catalog::portable`]).

use std::path::{Path, PathBuf};

use savescummer_catalog::Outcome;
pub use savescummer_catalog::portable::Portable;

use crate::model::Game;

pub trait GameRecords {
    /// A game record as stored.
    fn record(&self, game: &Game) -> String;
    /// A stored game record, ready to use.
    fn load(&self, data: &str) -> serde_json::Result<Game>;
}

impl GameRecords for Portable {
    fn record(&self, game: &Game) -> String {
        let mut game = game.clone();
        map_paths(&mut game, |path| PathBuf::from(self.contract(path)));
        serde_json::to_string(&game).expect("game serializes")
    }

    fn load(&self, data: &str) -> serde_json::Result<Game> {
        let mut game = Game::from_record_json(data)?;
        map_paths(&mut game, |path| self.expand(&path.to_string_lossy()));
        Ok(game)
    }
}

fn map_paths(game: &mut Game, map: impl Fn(&Path) -> PathBuf) {
    for install in &mut game.installs {
        install.install_dir = map(&install.install_dir);
        if let Some(prefix) = &mut install.proton_prefix {
            *prefix = map(prefix);
        }
    }
    for exe in game.catalog_executables.iter_mut().chain(&mut game.executable) {
        *exe = map(exe);
    }
    if let Some(Outcome::Resolved { save_set }) = &mut game.outcome {
        for target in save_set {
            target.root = map(&target.root);
        }
    }
    if let Some(location) = &mut game.location {
        location.text = map(Path::new(&location.text)).to_string_lossy().into_owned();
        location.real_root = map(&location.real_root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use savescummer_catalog::KnownFolders;

    fn portable(home: &str) -> Portable {
        Portable::new(&KnownFolders { home: Some(home.into()), ..Default::default() }, false)
    }

    #[cfg(unix)]
    #[test]
    fn a_stored_record_names_no_user_and_loads_on_another_pc() {
        let record = r#"{"id":"custom-1","kind":"custom","name":"Game","installed":true,
            "executable":"/Users/alex/Games/Game.app",
            "location":{"text":"/Users/alex/Library/Game/*.sav","real_root":"/Users/alex/Library/Game"}}"#;
        let stored = portable("/Users/alex").record(&portable("/Users/alex").load(record).unwrap());
        assert!(!stored.contains("alex"), "{stored}");
        let game = portable("/home/sam").load(&stored).unwrap();
        assert_eq!(game.executable.unwrap(), Path::new("/home/sam/Games/Game.app"));
        let location = game.location.unwrap();
        assert_eq!(location.text, "/home/sam/Library/Game/*.sav");
        assert_eq!(location.real_root, Path::new("/home/sam/Library/Game"));
    }
}
