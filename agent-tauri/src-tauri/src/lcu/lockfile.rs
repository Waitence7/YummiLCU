use std::path::{Path, PathBuf};

use crate::config::Config;

pub(crate) fn lockfile_path(config: &Config) -> Option<PathBuf> {
    if let Some(raw) = &config.lockfile_path {
        let path = expand_environment(raw);
        if Path::new(&path).exists() {
            return Some(PathBuf::from(path));
        }
    }

    [
        r"C:\Riot Games\League of Legends\lockfile",
        r"C:\Riot Games\League of Legends\Game\lockfile",
        r"%LOCALAPPDATA%\Riot Games\Riot Client\Config\lockfile",
    ]
    .iter()
    .map(|path| PathBuf::from(expand_environment(path)))
    .find(|path| path.exists())
}

fn expand_environment(value: &str) -> String {
    let mut expanded = value.to_owned();
    for (key, replacement) in std::env::vars() {
        expanded = expanded.replace(&format!("%{key}%"), &replacement);
    }
    expanded
}
