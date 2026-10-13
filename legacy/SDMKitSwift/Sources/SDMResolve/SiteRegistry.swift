import Foundation

/// One entry in the curated multi-site allowlist. Pure value type,
/// fixture-tested like `VerdictRules` / `PackageClustering` — tune the data,
/// not the control flow. Parent spec
/// `2026-09-03-multi-site-resolver-design.md` §4.
public struct SitePattern: Sendable, Equatable {
    public enum HostMatch: Sendable, Equatable {
        /// Matches this host exactly, plus its `www.` form.
        case exact(String)
        /// Matches any host ending with this (leading-dot) suffix, plus the
        /// bare apex. `.suffix(".twitch.tv")` matches `twitch.tv`,
        /// `www.twitch.tv`, `clips.twitch.tv`, …
        case suffix(String)
    }

    public var hosts: [HostMatch]
    /// URL path prefixes that mean "this is a playlist / channel / set".
    /// Feeds only the pre-check in `YtDlpResolver.looksLikePlaylist`; the
    /// parser still trusts yt-dlp's `_type`.
    public var playlistPathHints: [String]
    /// Per-site tokens spliced into every yt-dlp invocation for this site
    /// (e.g. `["--impersonate", "chrome"]`). Empty for almost every entry —
    /// the escape hatch for a site that needs a referer / impersonation.
    public var extraArgs: [String]

    public init(
        hosts: [HostMatch], playlistPathHints: [String] = [], extraArgs: [String] = []
    ) {
        self.hosts = hosts
        self.playlistPathHints = playlistPathHints
        self.extraArgs = extraArgs
    }

    func matches(host: String) -> Bool {
        hosts.contains { entry in
            switch entry {
            case .exact(let h):
                return host == h || host == "www." + h
            case .suffix(let s):
                return host == String(s.drop(while: { $0 == "." })) || host.hasSuffix(s)
            }
        }
    }
}

/// The curated allowlist. Code-only — there is no runtime extractor-list
/// matching and no user-editable field. Grouped by comment only; adult
/// sites carry no toggle and no runtime partition (spec §2).
public enum SiteRegistry {
    /// Common playlist/channel path prefixes shared by most sites.
    static let commonPlaylistHints = [
        "/playlist", "/@", "/channel/", "/c/", "/user/", "/sets/", "/album/",
        "/channels/", "/show/", "/showcase/",
    ]

    public static let patterns: [SitePattern] =
        generalVideo + live + social + audio
        + regional + broadcaster + adult

    // MARK: General video

    static let generalVideo: [SitePattern] = [
        SitePattern(
            hosts: [.exact("youtube.com"), .suffix(".youtube.com"), .exact("youtu.be")],
            playlistPathHints: commonPlaylistHints),
        SitePattern(
            hosts: [.exact("vimeo.com"), .suffix(".vimeo.com")],
            playlistPathHints: commonPlaylistHints),
        SitePattern(
            hosts: [.exact("dailymotion.com"), .suffix(".dailymotion.com"), .exact("dai.ly")],
            playlistPathHints: commonPlaylistHints),
        SitePattern(hosts: [.exact("rumble.com")], playlistPathHints: ["/c/", "/user/"]),
        SitePattern(hosts: [.exact("odysee.com")], playlistPathHints: ["/@"]),
        SitePattern(hosts: [.suffix(".bitchute.com"), .exact("bitchute.com")]),
        SitePattern(
            hosts: [.exact("bilibili.com"), .suffix(".bilibili.com")],
            playlistPathHints: ["/medialist/", "/festival/"]),
        SitePattern(hosts: [.exact("kick.com"), .suffix(".kick.com")]),
        SitePattern(hosts: [.exact("ok.ru"), .exact("odnoklassniki.ru")]),
        SitePattern(
            hosts: [.exact("vk.com"), .suffix(".vk.com"), .exact("vkvideo.ru")],
            playlistPathHints: ["/playlist/"]),
        SitePattern(hosts: [.exact("streamable.com")]),
        SitePattern(hosts: [.exact("loom.com"), .suffix(".loom.com")]),
        SitePattern(hosts: [.exact("ted.com"), .suffix(".ted.com")]),
        SitePattern(hosts: [.exact("nebula.tv"), .suffix(".nebula.tv")]),
    ]

    // MARK: Live / streaming

    static let live: [SitePattern] = [
        SitePattern(
            hosts: [.suffix(".twitch.tv"), .exact("twitch.tv")],
            playlistPathHints: ["/videos"])
    ]

    // MARK: Social

    static let social: [SitePattern] = [
        SitePattern(
            hosts: [.exact("tiktok.com"), .suffix(".tiktok.com")],
            playlistPathHints: ["/@"]),
        SitePattern(hosts: [.exact("instagram.com"), .suffix(".instagram.com")]),
        SitePattern(
            hosts: [.exact("facebook.com"), .suffix(".facebook.com"), .exact("fb.watch")]),
        SitePattern(
            hosts: [
                .exact("x.com"), .suffix(".x.com"), .exact("twitter.com"),
                .suffix(".twitter.com"),
            ]),
        SitePattern(
            hosts: [.exact("reddit.com"), .suffix(".reddit.com"), .exact("redd.it")],
            playlistPathHints: ["/r/"]),
        SitePattern(hosts: [.exact("bsky.app")]),
        SitePattern(hosts: [.exact("tumblr.com"), .suffix(".tumblr.com")]),
        SitePattern(hosts: [.exact("imgur.com"), .suffix(".imgur.com")]),
        SitePattern(hosts: [.exact("coub.com")]),
    ]

    // MARK: Audio

    static let audio: [SitePattern] = [
        SitePattern(
            hosts: [.exact("soundcloud.com"), .suffix(".soundcloud.com")],
            playlistPathHints: ["/sets/"]),
        SitePattern(
            hosts: [.exact("bandcamp.com"), .suffix(".bandcamp.com")],
            playlistPathHints: ["/album/"]),
        SitePattern(hosts: [.exact("mixcloud.com"), .suffix(".mixcloud.com")]),
    ]

    // MARK: Regional

    static let regional: [SitePattern] = [
        SitePattern(
            hosts: [.exact("aparat.com"), .suffix(".aparat.com")],
            playlistPathHints: ["/playlist/"])
    ]

    // MARK: Broadcaster (direct-URL, generally reliable)

    static let broadcaster: [SitePattern] = [
        SitePattern(hosts: [.suffix(".arte.tv"), .exact("arte.tv")]),
        SitePattern(hosts: [.suffix(".bbc.co.uk"), .suffix(".bbc.com")]),
        SitePattern(hosts: [.suffix(".pbs.org"), .exact("pbs.org")]),
    ]

    // MARK: Adult

    static let adult: [SitePattern] = [
        SitePattern(hosts: [.exact("xvideos.com"), .suffix(".xvideos.com")]),
        SitePattern(hosts: [.exact("xnxx.com"), .suffix(".xnxx.com")]),
        SitePattern(hosts: [.exact("xhamster.com"), .suffix(".xhamster.com")]),
        SitePattern(
            hosts: [.exact("pornhub.com"), .suffix(".pornhub.com")],
            playlistPathHints: ["/playlist/", "/model/", "/pornstar/"]),
        SitePattern(hosts: [.exact("youporn.com"), .suffix(".youporn.com")]),
        SitePattern(hosts: [.exact("redtube.com"), .suffix(".redtube.com")]),
        SitePattern(hosts: [.exact("spankbang.com"), .suffix(".spankbang.com")]),
        SitePattern(hosts: [.exact("eporner.com"), .suffix(".eporner.com")]),
    ]

    /// The first pattern whose host list matches `url`'s host, or `nil`.
    public static func match(_ url: URL) -> SitePattern? {
        guard let host = url.host?.lowercased() else { return nil }
        return patterns.first { $0.matches(host: host) }
    }
}
