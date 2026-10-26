//! What the native host does with each message the extension sends.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sdm_resolve::SiteRegistry;
use serde::Deserialize;
use serde_json::json;

use crate::cookies::{
    ChromeCookie, cookie_file_path, filter_cookies, render_netscape, write_cookie_file,
};
use crate::framing::{read_message, write_message};
use crate::status::write_status;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError(pub String);

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HostError {}

/// Starts a program with arguments and returns without waiting for it.
pub type Launcher = Box<dyn Fn(&Path, &[String]) -> std::io::Result<()> + Send + Sync>;

/// What the host reaches outside itself, injectable for tests.
pub struct HostEnvironment {
    /// SDM's data folder: holds the status file and `chrome-cookies.txt`.
    pub data_dir: PathBuf,
    /// The app to start for a captured download.
    pub app_executable: PathBuf,
    pub now: Box<dyn Fn() -> SystemTime + Send + Sync>,
    pub launch: Launcher,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Request {
    #[serde(rename = "extensionStatus")]
    ExtensionStatus { version: String },
    #[serde(rename = "download")]
    Download { url: String },
    #[serde(rename = "siteList")]
    SiteList,
    #[serde(rename = "cookies")]
    Cookies {
        #[serde(rename = "siteListVersion", default)]
        site_list_version: String,
        cookies: Vec<ChromeCookie>,
    },
}

/// Handles one request and returns the JSON reply.
pub fn handle(env: &HostEnvironment, request: &[u8]) -> Result<Vec<u8>, HostError> {
    let request: Request = serde_json::from_slice(request)
        .map_err(|error| HostError(format!("unreadable request: {error}")))?;
    let reply = match request {
        Request::ExtensionStatus { version } => {
            write_status(&env.data_dir, &version, (env.now)())
                .map_err(|e| HostError(format!("could not save the status: {e}")))?;
            json!({"success": true})
        }
        Request::Download { url } => {
            let parsed = url::Url::parse(&url)
                .map_err(|_| HostError("the download address is not a URL".into()))?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(HostError(
                    "only http and https downloads are handed over".into(),
                ));
            }
            (env.launch)(&env.app_executable, &["--add-url".to_owned(), url])
                .map_err(|e| HostError(format!("could not start SDM: {e}")))?;
            json!({"success": true})
        }
        Request::SiteList => json!({
            "success": true,
            "version": SiteRegistry::cookie_domains_version(),
            "domains": SiteRegistry::cookie_domains(),
        }),
        Request::Cookies {
            site_list_version,
            cookies,
        } => {
            let kept = filter_cookies(cookies);
            write_cookie_file(&cookie_file_path(&env.data_dir), &render_netscape(&kept))
                .map_err(|e| HostError(format!("could not write the cookie file: {e}")))?;
            let version = SiteRegistry::cookie_domains_version();
            let mut reply = json!({
                "success": true,
                "written": kept.len(),
                "siteListVersion": version,
            });
            if site_list_version != version {
                reply["domains"] = json!(SiteRegistry::cookie_domains());
            }
            reply
        }
    };
    serde_json::to_vec(&reply).map_err(|e| HostError(e.to_string()))
}

/// Reads one message from `input`, answers it on `output`, and returns the
/// process exit code. A failure is answered with `{"success":false}` when
/// possible; nothing but the reply is ever written to `output`, and no
/// cookie value is ever logged.
pub fn run(env: &HostEnvironment, input: &mut impl Read, output: &mut impl Write) -> i32 {
    let request = match read_message(input) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("SDM native host: {error:?}");
            return 1;
        }
    };
    let (reply, code) = match handle(env, &request) {
        Ok(reply) => (reply, 0),
        Err(error) => {
            eprintln!("SDM native host: {error}");
            (
                serde_json::to_vec(&json!({"success": false, "error": error.0}))
                    .unwrap_or_default(),
                1,
            )
        }
    };
    match write_message(output, &reply) {
        Ok(()) => code,
        Err(_) => 1,
    }
}
