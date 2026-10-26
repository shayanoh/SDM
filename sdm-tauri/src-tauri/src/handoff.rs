//! Chrome's extension reaches SDM two ways, both through this binary: Chrome
//! starts it as the native-messaging host (`run_native_host`), and the host
//! starts it again with `--add-url <url>` (`deliver` runs in the instance that
//! is, or becomes, the running app). Spec
//! `2026-10-26-tauri-browser-integration-design.md` §4.1, §4.2.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::SystemTime;

use sdm_app::App;
use sdm_browser::host::{HostEnvironment, run};
use tauri::AppHandle;

/// Chrome starts a native host with the calling extension's origin as the
/// first argument.
pub fn is_native_host_launch(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg.starts_with("chrome-extension://"))
}

/// The address after `--add-url`, if it is an http(s) URL.
pub fn add_url_from_args(args: &[String]) -> Option<String> {
    let position = args.iter().position(|arg| arg == "--add-url")?;
    let url = args.get(position + 1)?;
    let scheme_ok = url.starts_with("http://") || url.starts_with("https://");
    (scheme_ok && !url.chars().any(char::is_whitespace)).then(|| url.clone())
}

/// The program that starts SDM: the AppImage file itself when running from
/// one (its mount point changes on every launch), else this executable.
pub fn app_executable(appimage: Option<OsString>, current: PathBuf) -> PathBuf {
    match appimage {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => current,
    }
}

/// SDM's data folder, the same one `TauriHost` uses, without needing a
/// running Tauri app (the host runs before Tauri starts).
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("SDM"))
}

/// Runs one native-messaging exchange on stdin / stdout and returns the exit
/// code.
pub fn run_native_host() -> i32 {
    let (Some(data_dir), Ok(current)) = (data_dir(), std::env::current_exe()) else {
        eprintln!("SDM native host: could not find SDM's folders");
        return 1;
    };
    let env = HostEnvironment {
        data_dir,
        app_executable: app_executable(std::env::var_os("APPIMAGE"), current),
        now: Box::new(SystemTime::now),
        launch: Box::new(|program, arguments| {
            let mut command = std::process::Command::new(program);
            command
                .args(arguments)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                // DETACHED_PROCESS | CREATE_NO_WINDOW
                command.creation_flags(0x0000_0008 | 0x0800_0000);
            }
            command.spawn().map(|_| ())
        }),
    };
    run(
        &env,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
    )
}

/// Brings the main window forward on the Linkgrabber and ingests `url`
/// there, as the SwiftUI `com-shayanoh-sdm://download` handler does.
pub fn deliver(app: &AppHandle, core: std::sync::Arc<App>, url: String) {
    if let Err(error) = crate::main_window::show(app, Some("linkgrabber")) {
        tracing::warn!("could not show the running instance: {error}");
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = core.grabber_ingest_text(&url).await {
            tracing::warn!("could not add the captured download: {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn chrome_starts_the_host_with_the_extension_origin() {
        assert!(is_native_host_launch(&args(&[
            "sdm",
            "chrome-extension://fnnpcbmjpapeohfmpiefbocloodhlgjf/"
        ])));
        // Windows adds the parent window handle.
        assert!(is_native_host_launch(&args(&[
            "sdm.exe",
            "chrome-extension://fnnpcbmjpapeohfmpiefbocloodhlgjf/",
            "--parent-window=1234"
        ])));
        assert!(!is_native_host_launch(&args(&["sdm", "--hidden"])));
        assert!(!is_native_host_launch(&args(&["sdm"])));
    }

    #[test]
    fn add_url_takes_only_an_http_address_after_the_flag() {
        assert_eq!(
            add_url_from_args(&args(&["sdm", "--add-url", "https://a.com/x.zip"])),
            Some("https://a.com/x.zip".to_owned())
        );
        assert_eq!(
            add_url_from_args(&args(&["sdm", "--hidden", "--add-url", "http://a.com/x"])),
            Some("http://a.com/x".to_owned())
        );
        for bad in [
            args(&["sdm", "--add-url"]),
            args(&["sdm", "--add-url", "file:///etc/passwd"]),
            args(&["sdm", "--add-url", "https://a.com/a b"]),
            args(&["sdm", "https://a.com/x"]),
        ] {
            assert_eq!(add_url_from_args(&bad), None, "{bad:?}");
        }
    }

    #[test]
    fn an_appimage_is_started_by_its_own_path() {
        let current = PathBuf::from("/tmp/.mount_x/usr/bin/sdm");
        assert_eq!(
            app_executable(Some("/home/u/SDM.AppImage".into()), current.clone()),
            PathBuf::from("/home/u/SDM.AppImage")
        );
        assert_eq!(app_executable(None, current.clone()), current);
        assert_eq!(app_executable(Some("".into()), current.clone()), current);
    }
}
