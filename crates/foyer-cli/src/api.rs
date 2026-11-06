//! API wire types come from `foyer-types`; this module adds the bits the
//! CLI needs on top (argument parsing and display).

pub use foyer_types::{
    ApiError, Build, BuildState, ChannelStatus, ChunkUpload, CreateBuild, CreateBuildResponse,
    CreateTesterKey, CreatedTesterKey, LaunchTarget, Platform, TesterKey, UpdateChannel,
    UploadUrls, User,
};

/// clap value parser for `--platform`.
pub fn parse_platform(s: &str) -> Result<Platform, String> {
    Platform::parse(&s.to_ascii_lowercase())
        .ok_or_else(|| format!("unknown platform {s:?} (expected windows, macos, linux or web)"))
}

/// Channel names are short lowercase slugs such as `stable` or `beta`
/// (same rule as the API: 1 to 40 of `a-z`, `0-9` and `-`).
pub fn validate_channel(channel: &str) -> Result<(), String> {
    let ok = !channel.is_empty()
        && channel.len() <= 40
        && channel
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid channel {channel:?}: use 1-40 lowercase letters, digits and dashes"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_parses_case_insensitively() {
        assert_eq!(parse_platform("MacOS").unwrap(), Platform::Macos);
        assert!(parse_platform("amiga").is_err());
        assert_eq!(serde_json::to_string(&Platform::Web).unwrap(), "\"web\"");
    }

    #[test]
    fn channel_names() {
        for ok in ["stable", "beta", "windows-beta", "v2"] {
            validate_channel(ok).unwrap();
        }
        for bad in ["", "Beta", "a b", "a/b", "a_b", &"a".repeat(41)] {
            assert!(validate_channel(bad).is_err(), "{bad:?}");
        }
    }
}
