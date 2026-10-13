//! A link cannot choose where its file is written: the filename the grabber
//! derives is always one path component, whether it comes from the URL or
//! from a `Content-Disposition` header. Ordinary names are unchanged.
mod support;

use std::collections::HashMap;
use std::sync::Arc;

use sdm_grabber::{FakeProbeOrigin, FakeProbeOriginBehavior, LinkProber, ProbedLink};
use support::{su, within};

fn name_from_url(url: &str) -> String {
    ProbedLink::new(su(url)).effective_filename()
}

async fn name_from_header(disposition: &str) -> String {
    let url = "https://example.com/fallback.bin";
    let origin = Arc::new(FakeProbeOrigin::new());
    origin.set_behavior(
        FakeProbeOriginBehavior {
            status_code: 200,
            headers: HashMap::from([("content-disposition".to_owned(), disposition.to_owned())]),
            ..FakeProbeOriginBehavior::default()
        },
        &su(url),
    );
    let link = LinkProber::new(origin, false).probe(&su(url)).await;
    link.effective_filename()
}

#[test]
fn ordinary_url_names_are_unchanged() {
    let table = [
        ("https://example.com/movie.mp4", "movie.mp4"),
        ("https://example.com/a/b/My%20File.tar.gz", "My File.tar.gz"),
        ("https://example.com/%D9%86%D8%A7%D9%85.bin", "نام.bin"),
        ("https://example.com/a/.hidden", ".hidden"),
        (
            "https://example.com/many.dots.v1.2.3.bin",
            "many.dots.v1.2.3.bin",
        ),
        ("https://example.com/dir/", "dir"),
        ("https://example.com/a%20b/c%20d.txt", "c d.txt"),
    ];
    for (url, expected) in table {
        assert_eq!(name_from_url(url), expected, "{url}");
    }
    let long = format!("https://example.com/{}.bin", "a".repeat(200));
    assert_eq!(name_from_url(&long), format!("{}.bin", "a".repeat(200)));
}

#[test]
fn a_percent_encoded_slash_does_not_make_a_path() {
    // Foundation's lastPathComponent works on the decoded path.
    assert_eq!(
        name_from_url("https://host/x/%2FUsers%2Fme%2FLibrary%2FLaunchAgents%2Fa.plist"),
        "a.plist"
    );
    assert_eq!(name_from_url("https://host/x/a%2F..%2F..%2Fb.sh"), "b.sh");
    assert_eq!(name_from_url("https://host/%2Fetc%2Fpasswd"), "passwd");
}

#[test]
fn a_url_name_of_dots_falls_back_to_the_default() {
    // A bare host gives `/` as its last component (Foundation does too); that
    // must not reach a folder join either.
    for url in [
        "https://host/a/%2E%2E",
        "https://host/a/..",
        "https://host/a/%2E",
        "https://host/",
        "https://host",
    ] {
        let name = name_from_url(url);
        assert!(name != ".." && name != ".", "{url} gave {name:?}");
        assert!(
            !name.is_empty() && !name.contains('/'),
            "{url} gave {name:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn content_disposition_names_are_reduced_to_one_component() {
    within(async {
        assert_eq!(
            name_from_header("attachment; filename=\"My File.mp4\"").await,
            "My File.mp4"
        );
        assert_eq!(
            name_from_header("attachment; filename=\"/Users/me/Documents/x\"").await,
            "x"
        );
        assert_eq!(
            name_from_header("attachment; filename=\"../../x\"").await,
            "x"
        );
        // Nothing usable in the header: the URL's own name is used.
        assert_eq!(
            name_from_header("attachment; filename=\"..\"").await,
            "fallback.bin"
        );
        assert_eq!(name_from_header("attachment; filename=\"a/\"").await, "a");
    })
    .await;
}
