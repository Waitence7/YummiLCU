mod actions;
mod client;
mod events;
mod lockfile;
mod party;
mod rewards;
mod rofl;

pub(crate) use client::{LcuClient, LcuIdentity};
pub(crate) use events::LcuEventPoller;
pub(crate) use lockfile::{discover_lockfile, lockfile_path, LockfileDiscovery};
pub(crate) use rofl::{collect_replay_bundle, RoflMatchHint};

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
