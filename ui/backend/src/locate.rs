//! Where a handler skill or watch script comes from. The dispatcher and the source view both go
//! through here, so what the UI shows is the file that runs.
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{config::Project, pipeline::valid_board_name, workitem};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Board,
    Project,
    Bundled,
}

pub struct Found {
    pub path: PathBuf,
    pub origin: Origin,
}

/// Folders searched for `folder` (`skills` or `watch`), first match wins: the board's, the
/// project's, then `bundled`.
pub fn places(project: &Project, board: &str, folder: &str, bundled: PathBuf) -> Vec<(Origin, PathBuf)> {
    let own = workitem::workitems_dir(&project.repo);
    let mut places = Vec::new();
    if valid_board_name(board) {
        places.push((Origin::Board, own.join("boards").join(board).join(folder)));
    }
    places.push((Origin::Project, own.join(folder)));
    places.push((Origin::Bundled, bundled));
    places
}

/// The first of `places` that has the file `rel`.
pub fn find(places: &[(Origin, PathBuf)], rel: &Path) -> Option<Found> {
    places.iter().map(|(origin, place)| Found { path: place.join(rel), origin: *origin }).find(|found| found.path.is_file())
}

/// The folders, for "not found" messages.
pub fn looked(places: &[(Origin, PathBuf)]) -> String {
    places.iter().map(|(_, p)| p.display().to_string()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_place_with_the_file_wins() {
        let dir = tempfile::tempdir().unwrap();
        let (board, project, bundled) = (dir.path().join("b"), dir.path().join("p"), dir.path().join("w"));
        for place in [&board, &project, &bundled] {
            std::fs::create_dir_all(place).unwrap();
        }
        let places = vec![(Origin::Board, board.clone()), (Origin::Project, project.clone()), (Origin::Bundled, bundled.clone())];
        let rel = Path::new("x");
        assert!(find(&places, rel).is_none());
        std::fs::write(bundled.join("x"), "").unwrap();
        assert_eq!(find(&places, rel).unwrap().origin, Origin::Bundled);
        std::fs::write(project.join("x"), "").unwrap();
        assert_eq!(find(&places, rel).unwrap().origin, Origin::Project);
        std::fs::write(board.join("x"), "").unwrap();
        let found = find(&places, rel).unwrap();
        assert_eq!((found.origin, found.path), (Origin::Board, board.join("x")));
    }
}
