//! Toast notifications using WezTerm's toast-notification crate.

/// Show a persistent toast notification.
pub fn notify(title: &str, message: &str) {
    wezterm_toast_notification::persistent_toast_notification(title, message);
}

/// Show a toast with a clickable URL.
pub fn notify_with_url(title: &str, message: &str, url: &str) {
    wezterm_toast_notification::persistent_toast_notification_with_click_to_open_url(
        title, message, url,
    );
}
