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
    /// Registrable domains whose cookies yt-dlp needs for this site, written
    /// by hand: the site's own domains plus the ones its login lives on (the
    /// YouTube sign-in cookies are on `google.com`, which no YouTube host
    /// matches). A cookie belongs to the site when its domain equals one of
    /// these or is a subdomain of one. Grown by real use. Spec
    /// `2026-10-26-tauri-browser-integration-design.md` §4.5.
    pub cookie_domains: Vec<String>,
}

impl SitePattern {
    pub fn new(hosts: Vec<HostMatch>) -> Self {
        Self {
            hosts,
            playlist_path_hints: Vec::new(),
            extra_args: Vec::new(),
            cookie_domains: Vec::new(),
        }
    }

    pub fn with_cookie_domains(mut self, domains: &[&str]) -> Self {
        self.cookie_domains = domains.iter().map(|d| (*d).to_owned()).collect();
        self
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

    /// Every pattern's cookie domains: lowercase, sorted, de-duplicated.
    pub fn cookie_domains() -> Vec<String> {
        let mut domains: Vec<String> = Self::patterns()
            .iter()
            .flat_map(|p| p.cookie_domains.iter().map(|d| d.to_lowercase()))
            .collect();
        domains.sort();
        domains.dedup();
        domains
    }

    /// Whether a cookie with this `domain` (a leading dot allowed, any case)
    /// belongs to a listed site: equal to a cookie domain, or a subdomain of
    /// one. `notyoutube.com` and `youtube.com.evil.net` do not match.
    pub fn cookie_domain_matches(domain: &str) -> bool {
        let domain = domain.to_lowercase();
        let domain = domain.strip_prefix('.').unwrap_or(&domain);
        if domain.is_empty() {
            return false;
        }
        Self::patterns()
            .iter()
            .flat_map(|p| &p.cookie_domains)
            .any(|d| {
                domain == d.as_str()
                    || domain
                        .strip_suffix(d.as_str())
                        .is_some_and(|rest| rest.ends_with('.'))
            })
    }

    /// FNV-1a 64 of the domains joined with `\n`, as 16 lowercase hex digits.
    /// Stable across Rust versions, unlike `DefaultHasher`.
    pub fn version_of(domains: &[String]) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in domains.join("\n").bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{hash:016x}")
    }

    /// The version of [`Self::cookie_domains`].
    pub fn cookie_domains_version() -> String {
        Self::version_of(&Self::cookie_domains())
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
        .with_cookie_domains(&["youtube.com", "youtu.be", "google.com"])
        .with_playlist_path_hints(&COMMON_PLAYLIST_HINTS),
        SitePattern::new(vec![exact("vimeo.com"), suffix(".vimeo.com")])
            .with_cookie_domains(&["vimeo.com"])
            .with_playlist_path_hints(&COMMON_PLAYLIST_HINTS),
        SitePattern::new(vec![
            exact("dailymotion.com"),
            suffix(".dailymotion.com"),
            exact("dai.ly"),
        ])
        .with_cookie_domains(&["dailymotion.com", "dai.ly"])
        .with_playlist_path_hints(&COMMON_PLAYLIST_HINTS),
        SitePattern::new(vec![exact("rumble.com")])
            .with_cookie_domains(&["rumble.com"])
            .with_playlist_path_hints(&["/c/", "/user/"]),
        SitePattern::new(vec![exact("odysee.com")])
            .with_cookie_domains(&["odysee.com"])
            .with_playlist_path_hints(&["/@"]),
        SitePattern::new(vec![suffix(".bitchute.com"), exact("bitchute.com")])
            .with_cookie_domains(&["bitchute.com"]),
        SitePattern::new(vec![exact("bilibili.com"), suffix(".bilibili.com")])
            .with_cookie_domains(&["bilibili.com"])
            .with_playlist_path_hints(&["/medialist/", "/festival/"]),
        SitePattern::new(vec![exact("kick.com"), suffix(".kick.com")])
            .with_cookie_domains(&["kick.com"]),
        SitePattern::new(vec![exact("ok.ru"), exact("odnoklassniki.ru")])
            .with_cookie_domains(&["ok.ru", "odnoklassniki.ru"]),
        SitePattern::new(vec![
            exact("vk.com"),
            suffix(".vk.com"),
            exact("vkvideo.ru"),
        ])
        .with_cookie_domains(&["vk.com", "vkvideo.ru"])
        .with_playlist_path_hints(&["/playlist/"]),
        SitePattern::new(vec![exact("streamable.com")]).with_cookie_domains(&["streamable.com"]),
        SitePattern::new(vec![exact("loom.com"), suffix(".loom.com")])
            .with_cookie_domains(&["loom.com"]),
        SitePattern::new(vec![exact("ted.com"), suffix(".ted.com")])
            .with_cookie_domains(&["ted.com"]),
        SitePattern::new(vec![exact("nebula.tv"), suffix(".nebula.tv")])
            .with_cookie_domains(&["nebula.tv"]),
    ]
}

// MARK: Live / streaming

fn live() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![suffix(".twitch.tv"), exact("twitch.tv")])
            .with_cookie_domains(&["twitch.tv"])
            .with_playlist_path_hints(&["/videos"]),
    ]
}

// MARK: Social

fn social() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![exact("tiktok.com"), suffix(".tiktok.com")])
            .with_cookie_domains(&["tiktok.com"])
            .with_playlist_path_hints(&["/@"]),
        SitePattern::new(vec![exact("instagram.com"), suffix(".instagram.com")])
            .with_cookie_domains(&["instagram.com"]),
        SitePattern::new(vec![
            exact("facebook.com"),
            suffix(".facebook.com"),
            exact("fb.watch"),
        ])
        .with_cookie_domains(&["facebook.com", "fb.watch"]),
        SitePattern::new(vec![
            exact("x.com"),
            suffix(".x.com"),
            exact("twitter.com"),
            suffix(".twitter.com"),
        ])
        .with_cookie_domains(&["x.com", "twitter.com"]),
        SitePattern::new(vec![
            exact("reddit.com"),
            suffix(".reddit.com"),
            exact("redd.it"),
        ])
        .with_cookie_domains(&["reddit.com", "redd.it"])
        .with_playlist_path_hints(&["/r/"]),
        SitePattern::new(vec![exact("bsky.app")]).with_cookie_domains(&["bsky.app"]),
        SitePattern::new(vec![exact("tumblr.com"), suffix(".tumblr.com")])
            .with_cookie_domains(&["tumblr.com"]),
        SitePattern::new(vec![exact("imgur.com"), suffix(".imgur.com")])
            .with_cookie_domains(&["imgur.com"]),
        SitePattern::new(vec![exact("coub.com")]).with_cookie_domains(&["coub.com"]),
    ]
}

// MARK: Audio

fn audio() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![exact("soundcloud.com"), suffix(".soundcloud.com")])
            .with_cookie_domains(&["soundcloud.com"])
            .with_playlist_path_hints(&["/sets/"]),
        SitePattern::new(vec![exact("bandcamp.com"), suffix(".bandcamp.com")])
            .with_cookie_domains(&["bandcamp.com"])
            .with_playlist_path_hints(&["/album/"]),
        SitePattern::new(vec![exact("mixcloud.com"), suffix(".mixcloud.com")])
            .with_cookie_domains(&["mixcloud.com"]),
    ]
}

// MARK: Regional

fn regional() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![exact("aparat.com"), suffix(".aparat.com")])
            .with_cookie_domains(&["aparat.com"])
            .with_playlist_path_hints(&["/playlist/"]),
    ]
}

// MARK: Broadcaster (direct-URL, generally reliable)

fn broadcaster() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![suffix(".arte.tv"), exact("arte.tv")])
            .with_cookie_domains(&["arte.tv"]),
        SitePattern::new(vec![suffix(".bbc.co.uk"), suffix(".bbc.com")])
            .with_cookie_domains(&["bbc.co.uk", "bbc.com"]),
        SitePattern::new(vec![suffix(".pbs.org"), exact("pbs.org")])
            .with_cookie_domains(&["pbs.org"]),
    ]
}

// MARK: Adult

fn adult() -> Vec<SitePattern> {
    vec![
        SitePattern::new(vec![exact("xvideos.com"), suffix(".xvideos.com")])
            .with_cookie_domains(&["xvideos.com"]),
        SitePattern::new(vec![exact("xnxx.com"), suffix(".xnxx.com")])
            .with_cookie_domains(&["xnxx.com"]),
        SitePattern::new(vec![exact("xhamster.com"), suffix(".xhamster.com")])
            .with_cookie_domains(&["xhamster.com"]),
        SitePattern::new(vec![exact("pornhub.com"), suffix(".pornhub.com")])
            .with_cookie_domains(&["pornhub.com"])
            .with_playlist_path_hints(&["/playlist/", "/model/", "/pornstar/"]),
        SitePattern::new(vec![exact("youporn.com"), suffix(".youporn.com")])
            .with_cookie_domains(&["youporn.com"]),
        SitePattern::new(vec![exact("redtube.com"), suffix(".redtube.com")])
            .with_cookie_domains(&["redtube.com"]),
        SitePattern::new(vec![exact("spankbang.com"), suffix(".spankbang.com")])
            .with_cookie_domains(&["spankbang.com"]),
        SitePattern::new(vec![exact("eporner.com"), suffix(".eporner.com")])
            .with_cookie_domains(&["eporner.com"]),
    ]
}
