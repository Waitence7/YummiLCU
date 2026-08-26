use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::config::Config;

use super::client::LcuClient;

#[derive(Debug)]
pub(crate) struct LockfileDiscovery {
    pub path: Option<PathBuf>,
    pub diagnostics: Vec<String>,
    pub legacy_fallback: bool,
}

pub(crate) fn lockfile_path(config: &Config) -> Option<PathBuf> {
    discover_lockfile(config).path
}

pub(crate) fn discover_lockfile(config: &Config) -> LockfileDiscovery {
    if let Some(raw) = &config.lockfile_path {
        let path = PathBuf::from(expand_environment(raw));
        if valid_lcu_lockfile(&path) {
            return LockfileDiscovery { path: Some(path), diagnostics: Vec::new(), legacy_fallback: false };
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

    // Legacy discovery used Riot Client's local lockfile as an additional
    // candidate. Keep it only as a fallback so the stricter League paths win.
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local_app_data)
            .join("Riot Games")
            .join("Riot Client")
            .join("Config")
            .join("lockfile"));
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

    let mut diagnostics = Vec::new();
    let mut legacy_candidate = None;
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        match LcuClient::from_lockfile(&path) {
            Ok(_) => return LockfileDiscovery { path: Some(path), diagnostics, legacy_fallback: false },
            Err(error) => {
                diagnostics.push(format!("LCU lockfile 후보 검증 실패: {} ({error})", path.display()));
                // Preserve the pre-hardening behavior as a final fallback: if a
                // real lockfile file exists, return it and let the normal LCU
                // client path report/handle the validation failure explicitly.
                if legacy_candidate.is_none() && LcuClient::from_lockfile_legacy(&path).is_ok() {
                    legacy_candidate = Some(path);
                }
            }
        }
    }

    if let Some(path) = legacy_candidate {
        diagnostics.push(format!("LCU lockfile 과거 방식 fallback 사용: {}", path.display()));
        return LockfileDiscovery { path: Some(path), diagnostics, legacy_fallback: true };
    }

    LockfileDiscovery { path: None, diagnostics, legacy_fallback: false }
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
