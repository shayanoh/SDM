//! OS notifications for `sdm-app`'s [`Notification`]s, and routing a click on
//! one to the right view of the main window.

use sdm_app::host::HostError;
use sdm_app::notify::Notification;
use tauri::AppHandle;
use tauri_plugin_notification::{ActionPerformed, NotificationExt};

/// The title, body and web tab (the click route) of a notification.
pub fn payload(notification: &Notification) -> (String, String, &'static str) {
    let (title, body) = notification.title_body();
    (title, body, notification.route().as_str())
}

/// The web tab a click on a notification asks for, from the route it carries.
/// `None` when it carries none or an unknown one.
pub fn route_of(action: &ActionPerformed) -> Option<&'static str> {
    let route = action.notification()?.extra().get("route")?.as_str()?;
    ["downloads", "completed", "linkgrabber"]
        .into_iter()
        .find(|known| *known == route)
}

/// Shows `notification` as an OS notification that remembers its route.
pub fn show(app: &AppHandle, notification: &Notification) -> Result<(), HostError> {
    let (title, body, route) = payload(notification);
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .extra("route", route)
        .show()
        .map_err(|e| HostError(e.to_string()))
}

/// Brings the main window forward on the right tab when a notification is
/// clicked. Must run before the first notification is shown: the plugin
/// reports nothing for notifications shown before a handler existed.
pub fn install_click_handler(app: &AppHandle) -> tauri_plugin_notification::Result<()> {
    let handle = app.clone();
    app.notification().on_action(move |action| {
        if let Err(error) = crate::main_window::show(&handle, route_of(action)) {
            tracing::warn!("could not show the window for a notification click: {error}");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdm_app::notify::Notification;
    use tauri_plugin_notification::ActionPerformed;

    #[test]
    fn a_payload_carries_the_text_and_the_web_tab_of_its_route() {
        let finished = Notification::Finished {
            filename: "a.bin".into(),
        };
        assert_eq!(
            payload(&finished),
            ("Download finished".into(), "a.bin".into(), "completed")
        );
        let failed = Notification::Failed {
            filename: "a.bin".into(),
            reason: "404".into(),
        };
        assert_eq!(payload(&failed).2, "downloads");
        assert_eq!(
            payload(&Notification::LinksGrabbed { count: 2 }).2,
            "linkgrabber"
        );
    }

    fn performed(route: Option<&str>) -> ActionPerformed {
        let extra = match route {
            Some(route) => serde_json::json!({ "route": route }),
            None => serde_json::json!({}),
        };
        serde_json::from_value(serde_json::json!({
            "actionId": "tap",
            "notification": { "id": 1, "extra": extra }
        }))
        .expect("a valid action")
    }

    #[test]
    fn a_click_routes_to_the_tab_its_notification_named() {
        assert_eq!(route_of(&performed(Some("completed"))), Some("completed"));
        assert_eq!(
            route_of(&performed(Some("linkgrabber"))),
            Some("linkgrabber")
        );
        assert_eq!(route_of(&performed(Some("downloads"))), Some("downloads"));
    }

    #[test]
    fn a_click_with_no_or_an_unknown_route_goes_nowhere_in_particular() {
        assert_eq!(route_of(&performed(None)), None);
        assert_eq!(route_of(&performed(Some("settings"))), None);
    }
}
