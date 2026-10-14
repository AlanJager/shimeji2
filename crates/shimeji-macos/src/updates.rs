//! "Check for Updates…" opens the configured releases page without fetching it.

/// Set by the bundle builder from `packaging/repository.txt`.
pub fn releases_url() -> Option<&'static str> {
    option_env!("SHIMEJI_RELEASES_URL").filter(|url| !url.is_empty())
}
