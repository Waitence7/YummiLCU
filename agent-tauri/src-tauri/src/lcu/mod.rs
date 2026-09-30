mod actions;
mod client;
mod events;
mod fights;
mod history;
mod lockfile;
mod party;
mod remote_replay;
mod rewards;
mod rofl;

pub(crate) use client::{LcuClient, LcuIdentity};
pub(crate) use events::LcuEventPoller;
pub(crate) use fights::{
    analyze_replay_fights, attach_cached_match_fights, load_cached_match_fights,
    save_cached_match_fights,
};
pub(crate) use history::{empty_match_history, load_cached_match_history, save_match_history_page};
pub(crate) use lockfile::{discover_lockfile, lockfile_path, LockfileDiscovery};
pub(crate) use remote_replay::{
    remote_replay_analysis_job, remote_replay_analysis_viewer, start_remote_replay_analysis,
};
pub(crate) use rofl::{
    collect_replay_bundle, collect_replay_file, find_existing_replay_path, replay_for_viewer,
    RoflMatchHint,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LcuConnectionState {
    ClientStopped,
    LockfileFound,
    Connecting,
    Connected,
    LoggedIn,
    Error,
}

impl LcuConnectionState {
    pub(crate) const fn is_ready(self) -> bool {
        matches!(self, Self::LoggedIn)
    }
}
