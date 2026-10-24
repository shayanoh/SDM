//! The clipboard poll: reads the system clipboard every half second while
//! watching is on and hands the text to `sdm-app`, which decides what to grab.

use std::sync::Arc;
use std::time::Duration;

use sdm_app::{App, AppError};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// How often the clipboard is read. `NSPasteboard`-style change counters do
/// not exist on every platform, so every read is compared by `sdm-app`.
const PERIOD: Duration = Duration::from_millis(500);

/// Where clipboard text comes from.
pub trait TextSource: Send + Sync {
    /// The clipboard's text, or `None` when it is empty, not text, or cannot be read.
    fn read_text(&self) -> Option<String>;
}

struct SystemClipboard(AppHandle);

impl TextSource for SystemClipboard {
    fn read_text(&self) -> Option<String> {
        // Reading must not happen on the main thread (the plugin can deadlock
        // on Linux); callers run this on a blocking thread. A failure is not
        // logged with content, only that it happened.
        self.0
            .clipboard()
            .read_text()
            .ok()
            .filter(|text| !text.is_empty())
    }
}

/// One poll: nothing is read at all while watching is off.
pub async fn poll_once(core: &App, source: Arc<dyn TextSource>) -> Result<(), AppError> {
    if !core.clipboard_watching_enabled() {
        return Ok(());
    }
    let text = tokio::task::spawn_blocking(move || source.read_text())
        .await
        .unwrap_or(None);
    core.clipboard_changed(text).await
}

/// Starts the poll loop on the async runtime.
pub fn spawn(app: AppHandle, core: Arc<App>) {
    let source: Arc<dyn TextSource> = Arc::new(SystemClipboard(app));
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(PERIOD);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(error) = poll_once(&core, Arc::clone(&source)).await {
                tracing::warn!("clipboard poll failed: {error}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use sdm_app::App;
    use sdm_app::host::Host;
    use sdm_app::protocol::SettingsDto;
    use sdm_app::simulation::{SimulationHost, simulated_dependencies};

    struct CountingSource(AtomicUsize);

    impl TextSource for CountingSource {
        fn read_text(&self) -> Option<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some("nothing to grab here".to_owned())
        }
    }

    fn app(name: &str) -> Arc<App> {
        let root = std::env::temp_dir().join(format!("sdm-clip-{name}-{}", std::process::id()));
        let host: Arc<dyn Host> = Arc::new(SimulationHost::new(root));
        let dependencies = simulated_dependencies(host.as_ref(), 0, 0);
        App::new(host, dependencies).expect("an app")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_switched_off_watcher_never_reads_the_clipboard() {
        let app = app("off");
        app.update_settings(SettingsDto {
            clipboard_watching_enabled: false,
            ..SettingsDto::default()
        })
        .await
        .unwrap();
        let source = Arc::new(CountingSource(AtomicUsize::new(0)));
        poll_once(&app, source.clone()).await.unwrap();
        poll_once(&app, source.clone()).await.unwrap();
        assert_eq!(source.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_switched_on_watcher_reads_once_per_poll() {
        let app = app("on");
        let source = Arc::new(CountingSource(AtomicUsize::new(0)));
        poll_once(&app, source.clone()).await.unwrap();
        poll_once(&app, source.clone()).await.unwrap();
        assert_eq!(source.0.load(Ordering::SeqCst), 2);
    }
}
