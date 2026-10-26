use sdm_browser::prompt::{MAX_PROMPTS, PromptState, decide, load, save};

fn state(version: Option<&str>, count: u32) -> PromptState {
    PromptState {
        last_version: version.map(str::to_owned),
        shown_count: count,
    }
}

#[test]
fn a_fresh_install_prompts_and_counts_it() {
    let (show, next) = decide(&PromptState::default(), Some("2.0.0"), None, true);
    assert!(show);
    assert_eq!(next, state(Some("2.0.0"), 1));
}

#[test]
fn it_gives_up_after_three_prompts_for_the_same_version() {
    let mut current = PromptState::default();
    for _ in 0..MAX_PROMPTS {
        let (show, next) = decide(&current, Some("2.0.0"), None, true);
        assert!(show);
        current = next;
    }
    let (show, next) = decide(&current, Some("2.0.0"), None, true);
    assert!(!show);
    assert_eq!(next, current, "giving up changes nothing");
}

#[test]
fn a_new_bundled_version_starts_the_count_again() {
    let (show, next) = decide(&state(Some("2.0.0"), 3), Some("3.0.0"), Some("2.0.0"), true);
    assert!(show);
    assert_eq!(next, state(Some("3.0.0"), 1));
}

#[test]
fn an_up_to_date_extension_is_never_prompted_for() {
    let before = state(Some("2.0.0"), 1);
    let (show, next) = decide(&before, Some("2.0.0"), Some("2.0.0"), true);
    assert!(!show);
    assert_eq!(next, before);
}

#[test]
fn an_older_installed_version_is_prompted_for() {
    let (show, _) = decide(&PromptState::default(), Some("2.0.0"), Some("1.0.1"), true);
    assert!(show);
}

#[test]
fn no_chrome_or_no_bundled_extension_means_no_prompt() {
    let before = PromptState::default();
    assert!(!decide(&before, Some("2.0.0"), None, false).0);
    assert!(!decide(&before, None, None, true).0);
}

#[test]
fn the_count_survives_a_restart_and_a_damaged_file_starts_over() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load(dir.path()), PromptState::default());
    save(dir.path(), &state(Some("2.0.0"), 2)).unwrap();
    assert_eq!(load(dir.path()), state(Some("2.0.0"), 2));
    std::fs::write(dir.path().join("chrome-setup-prompt.json"), "not json").unwrap();
    assert_eq!(load(dir.path()), PromptState::default());
}
