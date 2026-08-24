mod actions;
mod client;
mod events;
mod lockfile;
mod party;
mod rewards;

pub(crate) use client::{LcuClient, LcuIdentity};
pub(crate) use events::LcuEventPoller;
pub(crate) use lockfile::lockfile_path;

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
