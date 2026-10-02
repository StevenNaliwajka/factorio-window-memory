//! positions.json: where each kind of window was last dragged to. It's plain JSON
//! so it can be edited by hand, e.g. to add a class to `ignore`.

use crate::geometry::Point;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Contents {
    #[serde(default)]
    version: u32,
    /// Window class name -> top-left corner the user last dragged it to.
    #[serde(default)]
    windows: BTreeMap<String, Point>,
    /// Window classes that should always open where Factorio puts them.
    #[serde(default)]
    ignore: BTreeSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoadOutcome {
    Missing,
    Loaded {
        windows: usize,
    },
    /// The file didn't parse; it was moved to `backup` and the store starts empty.
    Corrupt {
        backup: PathBuf,
        error: String,
    },
    Unreadable {
        error: String,
    },
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    contents: Contents,
    dirty: bool,
}

impl Store {
    pub fn load(path: impl Into<PathBuf>) -> (Store, LoadOutcome) {
        let path = path.into();
        let (contents, outcome) = match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                (Contents::default(), LoadOutcome::Missing)
            }
            Err(e) => (
                Contents::default(),
                LoadOutcome::Unreadable {
                    error: e.to_string(),
                },
            ),
            Ok(text) => match serde_json::from_str::<Contents>(&text) {
                Ok(contents) => {
                    let windows = contents.windows.len();
                    (contents, LoadOutcome::Loaded { windows })
                }
                Err(e) => {
                    let backup = path.with_extension("json.bad");
                    let _ = std::fs::rename(&path, &backup);
                    (
                        Contents::default(),
                        LoadOutcome::Corrupt {
                            backup,
                            error: e.to_string(),
                        },
                    )
                }
            },
        };
        (
            Store {
                path,
                contents,
                dirty: false,
            },
            outcome,
        )
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> usize {
        self.contents.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.contents.windows.is_empty()
    }

    pub fn get(&self, class: &str) -> Option<Point> {
        self.contents.windows.get(class).copied()
    }

    pub fn is_ignored(&self, class: &str) -> bool {
        self.contents.ignore.contains(class)
    }

    /// Returns true if this changed the stored position.
    pub fn set(&mut self, class: &str, pos: Point) -> bool {
        if self.get(class) == Some(pos) {
            return false;
        }
        self.contents.windows.insert(class.to_owned(), pos);
        self.dirty = true;
        true
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Write via a temp file and rename, so a crash mid-write can't truncate the file.
    pub fn save(&mut self) -> io::Result<()> {
        self.contents.version = FORMAT_VERSION;
        let text = serde_json::to_string_pretty(&self.contents).map_err(io::Error::other)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fwm-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_gives_an_empty_store() {
        let dir = temp_dir("missing");
        let (store, outcome) = Store::load(dir.join("positions.json"));
        assert_eq!(outcome, LoadOutcome::Missing);
        assert!(store.is_empty());
        assert!(!store.is_dirty());
    }

    #[test]
    fn positions_survive_a_save_and_reload() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("positions.json");
        let (mut store, _) = Store::load(&path);
        assert!(store.set("InventoryGui", Point { x: 10, y: 20 }));
        assert!(store.is_dirty());
        store.save().unwrap();
        assert!(!store.is_dirty());

        let (reloaded, outcome) = Store::load(&path);
        assert_eq!(outcome, LoadOutcome::Loaded { windows: 1 });
        assert_eq!(reloaded.get("InventoryGui"), Some(Point { x: 10, y: 20 }));
        assert!(!dir.join("positions.json.tmp").exists());
    }

    #[test]
    fn setting_the_same_position_is_not_a_change() {
        let dir = temp_dir("same");
        let (mut store, _) = Store::load(dir.join("positions.json"));
        store.set("TrainGui", Point { x: 1, y: 2 });
        store.save().unwrap();
        assert!(!store.set("TrainGui", Point { x: 1, y: 2 }));
        assert!(!store.is_dirty());
    }

    #[test]
    fn corrupt_file_is_backed_up_and_replaced() {
        let dir = temp_dir("corrupt");
        let path = dir.join("positions.json");
        std::fs::write(&path, "{ not json").unwrap();
        let (store, outcome) = Store::load(&path);
        assert!(store.is_empty());
        match outcome {
            LoadOutcome::Corrupt { backup, .. } => {
                assert_eq!(std::fs::read_to_string(backup).unwrap(), "{ not json");
            }
            other => panic!("expected Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn hand_written_file_with_an_ignore_list_loads() {
        let dir = temp_dir("ignore");
        let path = dir.join("positions.json");
        std::fs::write(
            &path,
            r#"{ "windows": { "InventoryGui": { "x": 5, "y": 6 } }, "ignore": ["TrainGui"] }"#,
        )
        .unwrap();
        let (store, outcome) = Store::load(&path);
        assert_eq!(outcome, LoadOutcome::Loaded { windows: 1 });
        assert!(store.is_ignored("TrainGui"));
        assert!(!store.is_ignored("InventoryGui"));
        assert_eq!(store.get("InventoryGui"), Some(Point { x: 5, y: 6 }));
    }
}
