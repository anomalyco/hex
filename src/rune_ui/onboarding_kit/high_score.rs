//! The practice game's best score: one number in `game-best` in Hex's data
//! folder, owner-only and replaced whole. It isn't a setting, so it stays
//! out of `settings.json` and `hex settings`.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const FILE: &str = "game-best";

/// The best score so far; 0 before the first.
pub(super) fn load() -> usize {
    crate::app_paths::support_dir().map_or(0, |dir| load_at(&dir))
}

/// Keeps `best` as the best score.
pub(super) fn save(best: usize) {
    let Ok(dir) = crate::app_paths::support_dir() else {
        return;
    };
    if let Err(error) = save_at(&dir, best) {
        tracing::warn!(%error, "could not save the practice game's best score");
    }
}

fn load_at(dir: &Path) -> usize {
    fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

fn save_at(dir: &Path, best: usize) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let temporary = dir.join(format!(".{FILE}.tmp"));
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temporary)?;
    writeln!(file, "{best}")?;
    drop(file);
    fs::rename(temporary, dir.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_best_score_round_trips_owner_only() {
        let dir = std::env::temp_dir().join(format!("hex-game-best-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(load_at(&dir), 0);
        save_at(&dir, 12).unwrap();
        save_at(&dir, 14).unwrap();
        assert_eq!(load_at(&dir), 14);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join(FILE)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        fs::write(dir.join(FILE), "not a number").unwrap();
        assert_eq!(load_at(&dir), 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
