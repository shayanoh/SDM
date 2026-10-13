use sdm_resolve::SiteRegistry;
use url::Url;

fn u(s: &str) -> Url {
    Url::parse(s).unwrap()
}

#[test]
fn site_registry_matches_expected() {
    let table = [
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
    ];
    for (url, expected) in table {
        assert_eq!(
            SiteRegistry::match_url(&u(url)).is_some(),
            expected,
            "{url}"
        );
    }
}

#[test]
fn exact_host_also_matches_www_but_not_arbitrary_subdomain() {
    assert!(SiteRegistry::match_url(&u("https://xnxx.com/v/1")).is_some());
    assert!(SiteRegistry::match_url(&u("https://www.xnxx.com/v/1")).is_some());
    // No `.suffix` entry would let a random left-label through for an
    // `.exact`-only host — but xnxx has a suffix entry, so pick a
    // genuinely exact-only one: streamable.
    assert!(SiteRegistry::match_url(&u("https://streamable.com/x")).is_some());
    assert!(SiteRegistry::match_url(&u("https://cdn.streamable.com/x")).is_none());
}

#[test]
fn playlist_hints_are_exposed_per_site() {
    let soundcloud = SiteRegistry::match_url(&u("https://soundcloud.com/a/sets/mix")).unwrap();
    assert!(soundcloud.playlist_path_hints.iter().any(|h| h == "/sets/"));
    let youtube = SiteRegistry::match_url(&u("https://www.youtube.com/playlist?list=x")).unwrap();
    assert!(youtube.playlist_path_hints.iter().any(|h| h == "/playlist"));
}

#[test]
fn extra_args_are_empty_by_default() {
    assert!(
        SiteRegistry::match_url(&u("https://vimeo.com/1"))
            .unwrap()
            .extra_args
            .is_empty()
    );
}
