use crate::error::{AppError, AppResult};
use url::Url;

/// Which platform a validated link belongs to. Used to pick an icon and a
/// small brand-tinted chip in the queue list so a mixed batch of Twitch
/// and YouTube links stays easy to scan at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Twitch,
    YouTube,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Twitch => "Twitch",
            Source::YouTube => "YouTube",
        }
    }
}

/// Validates a single token as either a Twitch Clip link or a YouTube
/// video/short link. Anything else — channel pages, playlists, the
/// homepage, non-clip Twitch URLs — is rejected so the queue only ever
/// contains links `yt-dlp` can turn into exactly one file.
pub fn validate_media_url(raw: &str) -> AppResult<(Url, Source)> {
    let url = Url::parse(raw.trim()).map_err(|_| AppError::InvalidUrl)?;
    if url.scheme() != "https" {
        return Err(AppError::InvalidUrl);
    }

    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let segments: Vec<&str> = url
        .path()
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();

    if is_twitch_clip(&host, &segments) {
        Ok((url, Source::Twitch))
    } else if is_youtube_video(&host, &segments, &url) {
        Ok((url, Source::YouTube))
    } else {
        Err(AppError::InvalidUrl)
    }
}

fn is_twitch_clip(host: &str, segments: &[&str]) -> bool {
    if !matches!(
        host,
        "twitch.tv" | "www.twitch.tv" | "m.twitch.tv" | "clips.twitch.tv"
    ) {
        return false;
    }

    if host == "clips.twitch.tv" {
        segments.len() == 1
    } else {
        (segments.len() == 3 && segments[1].eq_ignore_ascii_case("clip"))
            || (segments.len() == 2 && segments[0].eq_ignore_ascii_case("clip"))
    }
}

fn is_youtube_video(host: &str, segments: &[&str], url: &Url) -> bool {
    match host {
        "youtu.be" => segments.len() == 1 && !segments[0].is_empty(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => match segments
        {
            [first, second] if matches!(*first, "shorts" | "live" | "embed") => !second.is_empty(),
            [first] if *first == "watch" => url
                .query_pairs()
                .any(|(key, value)| key == "v" && !value.is_empty()),
            _ => false,
        },
        _ => false,
    }
}

/// A short, human-friendly label for a validated link — the clip slug or
/// video ID — so queue rows show something more useful than a raw URL.
pub fn media_display_name(raw: &str, source: Source) -> String {
    let Ok(url) = Url::parse(raw) else {
        return raw.to_string();
    };

    let id = match source {
        Source::YouTube => url
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value.into_owned())
            .or_else(|| last_path_segment(&url)),
        Source::Twitch => last_path_segment(&url),
    };

    id.filter(|segment| !segment.is_empty())
        .unwrap_or_else(|| raw.to_string())
}

fn last_path_segment(url: &Url) -> Option<String> {
    url.path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .map(|segment| segment.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Source, media_display_name, validate_media_url};

    #[test]
    fn accepts_twitch_clip_domains() {
        assert!(validate_media_url("https://clips.twitch.tv/ExampleClip").is_ok());
        assert!(validate_media_url("https://www.twitch.tv/streamer/clip/ExampleClip").is_ok());
        assert!(validate_media_url("https://www.twitch.tv/clip/ExampleClip").is_ok());
        assert!(validate_media_url("https://m.twitch.tv/streamer/clip/ExampleClip").is_ok());
    }

    #[test]
    fn accepts_twitch_trailing_slash() {
        assert!(validate_media_url("https://clips.twitch.tv/ExampleClip/").is_ok());
    }

    #[test]
    fn rejects_non_clip_twitch_urls() {
        assert!(validate_media_url("https://www.twitch.tv/streamer").is_err());
        assert!(validate_media_url("http://clips.twitch.tv/ExampleClip").is_err());
    }

    #[test]
    fn accepts_youtube_watch_links() {
        let (_, source) =
            validate_media_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
        assert_eq!(source, Source::YouTube);
    }

    #[test]
    fn accepts_youtube_short_links() {
        let (_, source) = validate_media_url("https://youtu.be/dQw4w9WgXcQ").unwrap();
        assert_eq!(source, Source::YouTube);
    }

    #[test]
    fn accepts_youtube_shorts_and_live() {
        assert!(validate_media_url("https://www.youtube.com/shorts/dQw4w9WgXcQ").is_ok());
        assert!(validate_media_url("https://www.youtube.com/live/dQw4w9WgXcQ").is_ok());
        assert!(validate_media_url("https://www.youtube.com/embed/dQw4w9WgXcQ").is_ok());
        assert!(validate_media_url("https://music.youtube.com/watch?v=dQw4w9WgXcQ").is_ok());
    }

    #[test]
    fn rejects_youtube_playlists_and_channels() {
        assert!(validate_media_url("https://www.youtube.com/playlist?list=PLabc").is_err());
        assert!(validate_media_url("https://www.youtube.com/watch?list=PLabc").is_err());
        assert!(validate_media_url("https://www.youtube.com/@SomeChannel").is_err());
        assert!(validate_media_url("https://www.youtube.com/").is_err());
    }

    #[test]
    fn rejects_unrelated_domains() {
        assert!(validate_media_url("https://example.com/watch?v=abc").is_err());
    }

    #[test]
    fn extracts_display_name_for_twitch() {
        assert_eq!(
            media_display_name("https://clips.twitch.tv/ExampleClip", Source::Twitch),
            "ExampleClip"
        );
        assert_eq!(
            media_display_name(
                "https://www.twitch.tv/streamer/clip/ExampleClip",
                Source::Twitch
            ),
            "ExampleClip"
        );
    }

    #[test]
    fn extracts_display_name_for_youtube() {
        assert_eq!(
            media_display_name(
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                Source::YouTube
            ),
            "dQw4w9WgXcQ"
        );
        assert_eq!(
            media_display_name("https://youtu.be/dQw4w9WgXcQ", Source::YouTube),
            "dQw4w9WgXcQ"
        );
        assert_eq!(
            media_display_name(
                "https://www.youtube.com/shorts/dQw4w9WgXcQ",
                Source::YouTube
            ),
            "dQw4w9WgXcQ"
        );
    }

    #[test]
    fn falls_back_to_raw_string_when_unparsable() {
        assert_eq!(media_display_name("not a url", Source::Twitch), "not a url");
    }
}
