use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::config::Config;

use super::client::LcuClient;

pub(crate) fn lockfile_path(config: &Config) -> Option<PathBuf> {
    if let Some(raw) = &config.lockfile_path {
        let path = PathBuf::from(expand_environment(raw));
        if valid_lcu_lockfile(&path) {
            return Some(path);
        }
    }

    let mut candidates = Vec::new();

    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        let metadata = PathBuf::from(program_data)
            .join("Riot Games")
            .join("Metadata")
            .join("league_of_legends.live")
            .join("league_of_legends.live.product_settings.yaml");
        if let Ok(contents) = fs::read_to_string(metadata) {
            if let Some(install_path) = product_install_path(&contents) {
                candidates.push(install_path.join("lockfile"));
                candidates.push(install_path.join("Game").join("lockfile"));
            }
        }
    }

    for drive in b'C'..=b'Z' {
        let drive = drive as char;
        candidates.extend([
            PathBuf::from(format!(r"{drive}:\Riot Games\League of Legends\lockfile")),
            PathBuf::from(format!(
                r"{drive}:\Riot Games\League of Legends\Game\lockfile"
            )),
            PathBuf::from(format!(
                r"{drive}:\Program Files\Riot Games\League of Legends\lockfile"
            )),
            PathBuf::from(format!(r"{drive}:\League of Legends\lockfile")),
        ]);
    }

    candidates.into_iter().find(|path| valid_lcu_lockfile(path))
}

fn valid_lcu_lockfile(path: &Path) -> bool {
    path.is_file() && LcuClient::from_lockfile(path).is_ok()
}

fn product_install_path(contents: &str) -> Option<PathBuf> {
    contents.lines().find_map(|line| {
        let value = line
            .trim()
            .strip_prefix("product_install_full_path:")?
            .trim()
            .trim_matches(['\'', '"']);
        (!value.is_empty()).then(|| PathBuf::from(value))
    })
}

fn expand_environment(value: &str) -> String {
    let mut expanded = value.to_owned();
    for (key, replacement) in std::env::vars() {
        expanded = expanded.replace(&format!("%{key}%"), &replacement);
    }
    expanded
}

#[cfg(test)]
mod tests {
    use super::product_install_path;
    use std::path::PathBuf;

    #[test]
    fn reads_riot_product_install_path() {
        let metadata = r#"
product_install_root: "D:/Riot Games"
product_install_full_path: "D:/Riot Games/League of Legends"
"#;

        assert_eq!(
            product_install_path(metadata),
            Some(PathBuf::from("D:/Riot Games/League of Legends"))
        );
    }
}
