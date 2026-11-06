//! The engine behind the Foyer desktop client, kept free of UI code so it
//! can be tested on its own.
//!
//! - [`api`]: anonymous client for the public API (catalog, channels,
//!   manifests, chunk download links, update checks). No login, ever.
//! - [`install`]: installs and updates a build into a folder, downloading
//!   only chunks that are not already on disk, with pause and resume.
//! - [`library`]: settings and the list of installed games.
//! - [`launch`]: finds a game's executable and starts it.
//! - [`client_update`]: checks for, verifies and installs new versions of
//!   the client itself.

pub mod api;
pub mod client_update;
pub mod install;
pub mod launch;
pub mod library;

pub use api::{Api, ApiFailure, DownloadEvent, GameQuery};
pub use install::{InstallError, InstallJob, Installed, JobHandle, Phase, Progress};
pub use library::{BuildRef, InstalledGame, Library, Settings};

use foyer_types::{ChannelStatus, Platform};

/// The channel a player gets by default for `platform`: the one named like
/// the platform (`windows`), then the shortest name (`linux` before
/// `linux-beta`), so stable channels win over test ones.
pub fn default_channel(channels: &[ChannelStatus], platform: Platform) -> Option<&ChannelStatus> {
    channels
        .iter()
        .filter(|c| c.platform == platform && c.latest.is_some())
        .min_by_key(|c| {
            let stable = c.channel == platform.as_str() || c.channel == "stable";
            (!stable, c.channel.len(), c.channel.clone())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use foyer_types::{Build, BuildState};

    fn channel(name: &str, platform: Platform, ready: bool) -> ChannelStatus {
        ChannelStatus {
            channel: name.into(),
            platform,
            latest: ready.then(|| Build {
                id: uuid::Uuid::nil(),
                game: "g".into(),
                channel: name.into(),
                platform,
                version: None,
                state: BuildState::Ready,
                total_size: 0,
                chunk_count: 0,
                created_at: chrono::Utc::now(),
                launch: None,
                scan: None,
                failure: None,
            }),
        }
    }

    #[test]
    fn prefers_the_stable_channel_for_the_platform() {
        let channels = [
            channel("linux-beta", Platform::Linux, true),
            channel("windows", Platform::Windows, true),
            channel("linux", Platform::Linux, true),
            channel("linux-x", Platform::Linux, false),
        ];
        let pick = |p| default_channel(&channels, p).map(|c| c.channel.as_str());
        assert_eq!(pick(Platform::Linux), Some("linux"));
        assert_eq!(pick(Platform::Windows), Some("windows"));
        assert_eq!(pick(Platform::Macos), None);
    }
}
