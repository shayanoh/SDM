use std::sync::OnceLock;

use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostMatch {
    /// Matches this host exactly, plus its `www.` form.
    Exact(String),
    /// Matches any host ending with this (leading-dot) suffix, plus the
    /// bare apex. `Suffix(".twitch.tv")` matches `twitch.tv`,
    /// `www.twitch.tv`, `clips.twitch.tv`, …
    Suffix(String),
}

fn exact(host: &str) -> HostMatch {
    HostMatch::Exact(host.to_owned())
}

fn suffix(host: &str) -> HostMatch {
    HostMatch::Suffix(host.to_owned())
}

/// One entry in the curated multi-site allowlist. Pure value type,
/// fixture-tested like `VerdictRules` / `PackageClustering` — tune the data,
/// not the control flow. Parent spec
/// `2026-09-03-multi-site-resolver-design.md` §4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitePattern {
    pub hosts: Vec<HostMatch>,
    /// URL path prefixes that mean "this is a playlist / channel / set".
    /// Feeds only the pre-check in `YtDlpResolver.looksLikePlaylist`; the
    /// parser still trusts yt-dlp's `_type`.
    pub playlist_path_hints: Vec<String>,
    /// Per-site tokens spliced into every yt-dlp invocation for this site
    /// (e.g. `["--impersonate", "chrome"]`). Empty for almost every entry —
    /// the escape hatch for a site that needs a referer / impersonation.
    pub extra_args: Vec<String>,
}

impl SitePattern {
    pub fn new(hosts: Vec<HostMatch>) -> Self {
        Self {
            hosts,
            playlist_path_hints: Vec::new(),
            extra_args: Vec::new(),
        }
    }

    pub fn with_playlist_path_hints(mut self, hints: &[&str]) -> Self {
        self.playlist_path_hints = hints.iter().map(|h| (*h).to_owned()).collect();
        self
    }

    pub fn with_extra_args(mut self, args: &[&str]) -> Self {
        self.extra_args = args.iter().map(|a| (*a).to_owned()).collect();
        self
    }

    /// `host` must already be lowercased.
    pub(crate) fn matches(&self, host: &str) -> bool {
        self.hosts.iter().any(|entry| match entry {
            HostMatch::Exact(h) => host == h || host.strip_prefix("www.") == Some(h.as_str()),
            HostMatch::Suffix(s) => host == s.trim_start_matches('.') || host.ends_with(s.as_str()),
        })
    }
}

/// The curated allowlist. Code-only — there is no runtime extractor-list
/// matching and no user-editable field. Grouped by comment only; adult
/// sites carry no toggle and no runtime partition (spec §2).
pub enum SiteRegistry {}

/// Common playlist/channel path prefixes shared by most sites.
pub(crate) const COMMON_PLAYLIST_HINTS: [&str; 10] = [
    "/playlist",
    "/@",
    "/channel/",
    "/c/",
    "/user/",
    "/sets/",
    "/album/",
    "/channels/",
    "/show/",
    "/showcase/",
];

impl SiteRegistry {
    pub fn patterns() -> &'static [SitePattern] {
        static PATTERNS: OnceLock<Vec<SitePattern>> = OnceLock::new();
        PATTERNS.get_or_init(|| {
            [
                general_video(),
                live(),
                social(),
                audio(),
                regional(),
                broadcaster(),
                adult(),
            ]
            .concat()
        })
    }

    /// The first pattern whose host list matches `url`'s host, or `None`.
    /// (`match` in the Swift; a Rust keyword here.)
    pub fn match_url(url: &Url) -> Option<&'static SitePattern> {
        let host = url.host_str()?.to_lowercase();
        Self::patterns().iter().find(|p| p.matches(&host))
    }
}

// MARK: General video

fn general_video() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![
            exact("youtube.com"),
            suffix(".youtube.com"),
            exact("youtu.be"),
        ])
        .with_playlist_path_hints(&COMMON_PLAYLIST_HINTS),
        SitePattern::new(vec![exact("vimeo.com"), suffix(".vimeo.com")])
            .with_playlist_path_hints(&COMMON_PLAYLIST_HINTS),
        SitePattern::new(vec![
            exact("dailymotion.com"),
            suffix(".dailymotion.com"),
            exact("dai.ly"),
        ])
        .with_playlist_path_hints(&COMMON_PLAYLIST_HINTS),
        SitePattern::new(vec![exact("rumble.com")]).with_playlist_path_hints(&["/c/", "/user/"]),
        SitePattern::new(vec![exact("odysee.com")]).with_playlist_path_hints(&["/@"]),
        SitePattern::new(vec![suffix(".bitchute.com"), exact("bitchute.com")]),
        SitePattern::new(vec![exact("bilibili.com"), suffix(".bilibili.com")])
            .with_playlist_path_hints(&["/medialist/", "/festival/"]),
        SitePattern::new(vec![exact("kick.com"), suffix(".kick.com")]),
        SitePattern::new(vec![exact("ok.ru"), exact("odnoklassniki.ru")]),
        SitePattern::new(vec![
            exact("vk.com"),
            suffix(".vk.com"),
            exact("vkvideo.ru"),
        ])
        .with_playlist_path_hints(&["/playlist/"]),
        SitePattern::new(vec![exact("streamable.com")]),
        SitePattern::new(vec![exact("loom.com"), suffix(".loom.com")]),
        SitePattern::new(vec![exact("ted.com"), suffix(".ted.com")]),
        SitePattern::new(vec![exact("nebula.tv"), suffix(".nebula.tv")]),
    ]
}

// MARK: Live / streaming

fn live() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![suffix(".twitch.tv"), exact("twitch.tv")])
            .with_playlist_path_hints(&["/videos"]),
    ]
}

// MARK: Social

fn social() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![exact("tiktok.com"), suffix(".tiktok.com")])
            .with_playlist_path_hints(&["/@"]),
        SitePattern::new(vec![exact("instagram.com"), suffix(".instagram.com")]),
        SitePattern::new(vec![
            exact("facebook.com"),
            suffix(".facebook.com"),
            exact("fb.watch"),
        ]),
        SitePattern::new(vec![
            exact("x.com"),
            suffix(".x.com"),
            exact("twitter.com"),
            suffix(".twitter.com"),
        ]),
        SitePattern::new(vec![
            exact("reddit.com"),
            suffix(".reddit.com"),
            exact("redd.it"),
        ])
        .with_playlist_path_hints(&["/r/"]),
        SitePattern::new(vec![exact("bsky.app")]),
        SitePattern::new(vec![exact("tumblr.com"), suffix(".tumblr.com")]),
        SitePattern::new(vec![exact("imgur.com"), suffix(".imgur.com")]),
        SitePattern::new(vec![exact("coub.com")]),
    ]
}

// MARK: Audio

fn audio() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![exact("soundcloud.com"), suffix(".soundcloud.com")])
            .with_playlist_path_hints(&["/sets/"]),
        SitePattern::new(vec![exact("bandcamp.com"), suffix(".bandcamp.com")])
            .with_playlist_path_hints(&["/album/"]),
        SitePattern::new(vec![exact("mixcloud.com"), suffix(".mixcloud.com")]),
    ]
}

// MARK: Regional

fn regional() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![exact("aparat.com"), suffix(".aparat.com")])
            .with_playlist_path_hints(&["/playlist/"]),
    ]
}

// MARK: Broadcaster (direct-URL, generally reliable)

fn broadcaster() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![suffix(".arte.tv"), exact("arte.tv")]),
        SitePattern::new(vec![suffix(".bbc.co.uk"), suffix(".bbc.com")]),
        SitePattern::new(vec![suffix(".pbs.org"), exact("pbs.org")]),
    ]
}

// MARK: Adult

fn adult() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![exact("xvideos.com"), suffix(".xvideos.com")]),
        SitePattern::new(vec![exact("xnxx.com"), suffix(".xnxx.com")]),
        SitePattern::new(vec![exact("xhamster.com"), suffix(".xhamster.com")]),
        SitePattern::new(vec![exact("pornhub.com"), suffix(".pornhub.com")])
            .with_playlist_path_hints(&["/playlist/", "/model/", "/pornstar/"]),
        SitePattern::new(vec![exact("youporn.com"), suffix(".youporn.com")]),
        SitePattern::new(vec![exact("redtube.com"), suffix(".redtube.com")]),
        SitePattern::new(vec![exact("spankbang.com"), suffix(".spankbang.com")]),
        SitePattern::new(vec![exact("eporner.com"), suffix(".eporner.com")]),
    ]
}
