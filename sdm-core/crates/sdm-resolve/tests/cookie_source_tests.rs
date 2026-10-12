use sdm_resolve::CookieSource;

#[test]
fn none_produces_no_arguments() {
    assert_eq!(CookieSource::None.yt_dlp_arguments(), Vec::<String>::new());
}

#[test]
fn chrome_produces_cookies_from_browser_argument() {
    assert_eq!(
        CookieSource::Chrome.yt_dlp_arguments(),
        vec!["--cookies-from-browser", "chrome"]
    );
}

#[test]
fn every_browser_case_has_a_raw_value() {
    for source in CookieSource::ALL {
        if source != CookieSource::None {
            assert_eq!(source.yt_dlp_arguments().len(), 2);
        }
    }
}
