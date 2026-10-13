import Foundation
import Testing

@testable import SDMResolve

private func u(_ s: String) -> URL { URL(string: s)! }

@Test(
    arguments: [
        ("https://www.youtube.com/watch?v=a", true),
        ("https://youtu.be/a", true),
        ("https://music.youtube.com/watch?v=a", true),
        ("https://vimeo.com/12345", true),
        ("https://player.vimeo.com/video/12345", true),
        ("https://www.dailymotion.com/video/x1", true),
        ("https://www.tiktok.com/@x/video/1", true),
        ("https://vm.tiktok.com/ZM1/", true),
        ("https://clips.twitch.tv/Foo", true),
        ("https://www.twitch.tv/videos/123", true),
        ("https://soundcloud.com/artist/track", true),
        ("https://on.soundcloud.com/abc", true),
        ("https://artist.bandcamp.com/album/thing", true),
        ("https://www.aparat.com/v/abc", true),
        ("https://www.xvideos.com/video123/x", true),
        ("https://www.xnxx.com/video-x/y", true),
        ("https://xhamster.com/videos/x", true),
        ("https://www.pornhub.com/view_video.php?viewkey=1", true),
        ("https://www.youporn.com/watch/1/x/", true),
        ("https://www.reddit.com/r/x/comments/y/z/", true),
        ("https://x.com/user/status/1", true),
        ("https://twitter.com/user/status/1", true),
        ("https://fb.watch/abc/", true),
        ("https://www.instagram.com/reel/abc/", true),
        ("https://example.com/video.mp4", false),
        ("https://youtube.com.evil.com/x", false),
        ("https://notyoutube.com/x", false),
        ("https://google.com/", false),
        ("https://en.wikipedia.org/wiki/x", false),
    ]
)
func siteRegistryMatchesExpected(_ urlString: String, _ expected: Bool) {
    #expect((SiteRegistry.match(u(urlString)) != nil) == expected)
}

@Test func exactHostAlsoMatchesWwwButNotArbitrarySubdomain() {
    #expect(SiteRegistry.match(u("https://xnxx.com/v/1")) != nil)
    #expect(SiteRegistry.match(u("https://www.xnxx.com/v/1")) != nil)
    // No `.suffix` entry would let a random left-label through for an
    // `.exact`-only host — but xnxx has a suffix entry, so pick a
    // genuinely exact-only one: streamable.
    #expect(SiteRegistry.match(u("https://streamable.com/x")) != nil)
    #expect(SiteRegistry.match(u("https://cdn.streamable.com/x")) == nil)
}

@Test func playlistHintsAreExposedPerSite() {
    #expect(
        SiteRegistry.match(u("https://soundcloud.com/a/sets/mix"))?
            .playlistPathHints.contains("/sets/") == true)
    #expect(
        SiteRegistry.match(u("https://www.youtube.com/playlist?list=x"))?
            .playlistPathHints.contains("/playlist") == true)
}

@Test func extraArgsAreEmptyByDefault() {
    #expect(SiteRegistry.match(u("https://vimeo.com/1"))?.extraArgs.isEmpty == true)
}
