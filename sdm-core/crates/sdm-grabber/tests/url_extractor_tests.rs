mod support;

use sdm_grabber::UrlExtractor;
use support::su;

#[test]
fn extracts_plain_url_from_prose() {
    let text = "Check this out: https://example.com/movie.mp4 nice right";
    assert_eq!(
        UrlExtractor::extract_links(text),
        vec![su("https://example.com/movie.mp4")]
    );
}

#[test]
fn extracts_multiple_distinct_urls_in_order() {
    let text = "https://a.example.com/1.zip and https://b.example.com/2.zip";
    assert_eq!(
        UrlExtractor::extract_links(text),
        vec![
            su("https://a.example.com/1.zip"),
            su("https://b.example.com/2.zip"),
        ]
    );
}

#[test]
fn dedupes_repeated_url() {
    let text = "https://a.example.com/1.zip mirror: https://a.example.com/1.zip";
    assert_eq!(
        UrlExtractor::extract_links(text),
        vec![su("https://a.example.com/1.zip")]
    );
}

#[test]
fn ignores_non_http_schemes() {
    let text = "ftp://old.example.com/file.zip or mailto:me@example.com";
    assert!(UrlExtractor::extract_links(text).is_empty());
}

#[test]
fn handles_url_on_its_own_line_within_a_paragraph() {
    let text = "Here's the download link:\nhttps://cdn.example.com/season1/show.s01e01.mkv\nLet me know if it works.";
    assert_eq!(
        UrlExtractor::extract_links(text),
        vec![su("https://cdn.example.com/season1/show.s01e01.mkv")]
    );
}

#[test]
fn returns_empty_for_text_with_no_links() {
    assert!(UrlExtractor::extract_links("just some text, nothing to see").is_empty());
}

#[test]
fn ignores_bare_host_urls_with_no_path_or_query() {
    let text = "7.3.2.example.org\nhttps://something.com/\nhttp://another.com\nvisit https://real.example/file.zip for the download\nhttps://cdn.example.com/?token=abc123\nhttps://www.youtube.com/@SomeCreator";
    assert_eq!(
        UrlExtractor::extract_links(text),
        vec![
            su("https://real.example/file.zip"),
            su("https://cdn.example.com/?token=abc123"),
            su("https://www.youtube.com/@SomeCreator"),
        ]
    );
}

#[test]
fn is_grabbable_rules() {
    assert!(!UrlExtractor::is_grabbable(&su("https://something.com")));
    assert!(!UrlExtractor::is_grabbable(&su("https://something.com/")));
    assert!(!UrlExtractor::is_grabbable(&su("http://7.3.2.example.org")));
    assert!(UrlExtractor::is_grabbable(&su("https://x.com/f.mp4")));
    assert!(UrlExtractor::is_grabbable(&su("https://x.com/?id=1")));
    assert!(UrlExtractor::is_grabbable(&su(
        "https://www.youtube.com/watch?v=abc"
    )));
}
