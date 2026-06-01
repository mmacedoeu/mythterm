//! URL opening using WezTerm's open-url crate.

/// Open a URL in the default browser.
pub fn open_url(url: &str) {
    wezterm_open_url::open_url(url);
}

/// Open a URL with a specific application.
pub fn open_with(url: &str, app: &str) {
    wezterm_open_url::open_with(url, app);
}
