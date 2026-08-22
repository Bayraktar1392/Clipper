use crate::error::{AppError, AppResult};
use url::Url;

pub fn validate_clip_url(raw: &str) -> AppResult<Url> {
    let url = Url::parse(raw.trim()).map_err(|_| AppError::InvalidUrl)?;
    if url.scheme() != "https" {
        return Err(AppError::InvalidUrl);
    }

    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();

    if !matches!(
        host.as_str(),
        "twitch.tv" | "www.twitch.tv" | "m.twitch.tv" | "clips.twitch.tv"
    ) {
        return Err(AppError::InvalidUrl);
    }

    let parts: Vec<&str> = url
        .path()
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let valid = if host == "clips.twitch.tv" {
        parts.len() == 1
    } else {
        (parts.len() == 3 && parts[1].eq_ignore_ascii_case("clip"))
            || (parts.len() == 2 && parts[0].eq_ignore_ascii_case("clip"))
    };

    if valid {
        Ok(url)
    } else {
        Err(AppError::InvalidUrl)
    }
}

/// A short, human-friendly label for a validated clip URL: the clip slug
/// itself, so queue rows can show something more useful than a raw link.
pub fn clip_display_name(raw: &str) -> String {
    Url::parse(raw)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|segments| segments.filter(|segment| !segment.is_empty()).next_back())
                .map(|segment| segment.to_string())
        })
        .filter(|segment| !segment.is_empty())
        .unwrap_or_else(|| raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::{clip_display_name, validate_clip_url};

    #[test]
    fn accepts_clip_domains() {
        assert!(validate_clip_url("https://clips.twitch.tv/ExampleClip").is_ok());
        assert!(validate_clip_url("https://www.twitch.tv/streamer/clip/ExampleClip").is_ok());
        assert!(validate_clip_url("https://www.twitch.tv/clip/ExampleClip").is_ok());
    }

    #[test]
    fn accepts_trailing_slash() {
        assert!(validate_clip_url("https://clips.twitch.tv/ExampleClip/").is_ok());
    }

    #[test]
    fn rejects_non_clip_urls() {
        assert!(validate_clip_url("https://www.twitch.tv/streamer").is_err());
        assert!(validate_clip_url("http://clips.twitch.tv/ExampleClip").is_err());
        assert!(validate_clip_url("https://example.com/clip/ExampleClip").is_err());
    }

    #[test]
    fn extracts_display_name_from_clips_domain() {
        assert_eq!(
            clip_display_name("https://clips.twitch.tv/ExampleClip"),
            "ExampleClip"
        );
    }

    #[test]
    fn extracts_display_name_from_channel_url() {
        assert_eq!(
            clip_display_name("https://www.twitch.tv/streamer/clip/ExampleClip"),
            "ExampleClip"
        );
    }

    #[test]
    fn extracts_display_name_with_trailing_slash() {
        assert_eq!(
            clip_display_name("https://clips.twitch.tv/ExampleClip/"),
            "ExampleClip"
        );
    }

    #[test]
    fn falls_back_to_raw_string_when_unparsable() {
        assert_eq!(clip_display_name("not a url"), "not a url");
    }
}
