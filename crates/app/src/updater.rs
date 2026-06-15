//! Auto-update via Sparkle (the macOS standard updater), reached through the
//! Objective-C runtime so the native Rust/GPUI app can drive it without a
//! Cocoa host.
//!
//! [`start`] creates Sparkle's `SPUStandardUpdaterController`, which begins its
//! own scheduled background checks against the appcast feed configured in the
//! bundle's Info.plist (`SUFeedURL` + `SUPublicEDKey`, with
//! `SUEnableAutomaticChecks` / `SUAutomaticallyUpdate` for silent staging).
//! [`check_for_updates`] triggers a user-initiated check. Both must run on the
//! main thread; the controller is retained for the process lifetime.
//!
//! The feed and signature key live in the bundle, so updates only work from a
//! packaged `DaisyNotes.app` — a bare `cargo run` has no Info.plist for Sparkle
//! to read, so `start` is effectively a no-op there.

#[cfg(target_os = "macos")]
// The Sparkle bridge is unavoidably `unsafe`: it sends Objective-C messages
// through the runtime. Kept to this module, every call audited against the
// SPUStandardUpdaterController API.
#[allow(unsafe_code)]
mod imp {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    /// Retained `SPUStandardUpdaterController` pointer (as bits; main-thread
    /// only). `0` means "not started" / Sparkle unavailable.
    static CONTROLLER: AtomicUsize = AtomicUsize::new(0);

    /// True only when running as `…/Foo.app/Contents/MacOS/Foo`. Sparkle reads
    /// its feed and key from that bundle's Info.plist; the bare binary from
    /// `cargo run` has neither, and starting the controller there pops a
    /// "failed to start" dialog — so we skip it entirely in development.
    fn in_app_bundle() -> bool {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.into_os_string().into_string().ok())
            .is_some_and(|p| p.contains(".app/Contents/MacOS/"))
    }

    pub fn start() {
        if CONTROLLER.load(Ordering::Relaxed) != 0 {
            return;
        }
        if !in_app_bundle() {
            tracing::info!("not a packaged .app; auto-update disabled (dev run)");
            return;
        }
        let Some(cls) = AnyClass::get(c"SPUStandardUpdaterController") else {
            tracing::warn!("Sparkle framework not loaded; auto-update disabled");
            return;
        };
        // [[SPUStandardUpdaterController alloc]
        //     initWithStartingUpdater:YES updaterDelegate:nil userDriverDelegate:nil]
        let controller: *mut AnyObject = unsafe {
            let alloc: *mut AnyObject = msg_send![cls, alloc];
            msg_send![
                alloc,
                initWithStartingUpdater: Bool::YES,
                updaterDelegate: std::ptr::null_mut::<AnyObject>(),
                userDriverDelegate: std::ptr::null_mut::<AnyObject>()
            ]
        };
        if controller.is_null() {
            tracing::warn!("SPUStandardUpdaterController init returned nil");
            return;
        }
        CONTROLLER.store(controller as usize, Ordering::Relaxed);
        tracing::info!("Sparkle updater started");
    }

    pub fn check_for_updates() {
        let ptr = CONTROLLER.load(Ordering::Relaxed) as *mut AnyObject;
        if ptr.is_null() {
            tracing::warn!("check_for_updates before updater started");
            return;
        }
        // -[SPUStandardUpdaterController checkForUpdates:]
        unsafe {
            let _: () = msg_send![ptr, checkForUpdates: std::ptr::null_mut::<AnyObject>()];
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn start() {}
    pub fn check_for_updates() {}
}

/// Start Sparkle's updater (scheduled background checks). Call once on the main
/// thread after the app has launched.
pub fn start() {
    imp::start();
}

/// Trigger a user-initiated update check (Sparkle's standard UI). Main thread.
pub fn check_for_updates() {
    imp::check_for_updates();
}

// ── Appcast version check ───────────────────────────────────────────────────
// A lightweight read of the feed, used for the instant in-pane verdict and the
// background poll that raises the topbar pill. Sparkle still owns the actual
// download/verify/install — this only decides what to *show*.

/// The Sparkle appcast the bundle's `SUFeedURL` points at.
pub(crate) const APPCAST_URL: &str =
    "https://github.com/appsicle/daisynotes/releases/download/updates/appcast.xml";

/// This build's version. `DAISYNOTES_UPDATE_BASELINE` overrides it for local
/// testing — set it below the published feed version to make the real feed read
/// as "newer" and exercise the update pill without faking a release.
pub(crate) fn current_version() -> String {
    std::env::var("DAISYNOTES_UPDATE_BASELINE")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string())
}

/// Fetch the appcast (blocking; call on a background thread). `None` on any
/// network/HTTP error — including the 404 before the feed is published.
pub(crate) fn fetch_appcast() -> Option<String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?
        .get(APPCAST_URL)
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .ok()
}

/// The newest item's version in the feed: the first
/// `<sparkle:shortVersionString>` (or `<sparkle:version>`).
pub(crate) fn latest_version(xml: &str) -> Option<String> {
    for tag in ["sparkle:shortVersionString", "sparkle:version"] {
        let open = format!("<{tag}>");
        if let Some(start) = xml.find(&open) {
            let rest = &xml[start + open.len()..];
            if let Some(end) = rest.find(&format!("</{tag}>")) {
                let value = rest[..end].trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Dotted-numeric "is `a` strictly newer than `b`" (e.g. `0.1.3` > `0.1.2`).
pub(crate) fn version_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| {
        s.split('.')
            .map(|p| p.trim().parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Fetch + parse + compare in one blocking call: `Some(version)` when the feed
/// offers something newer than this build, `None` when up to date or on error.
/// (Settings distinguishes up-to-date from error itself; the background poll
/// only cares whether to raise the pill.)
pub(crate) fn newer_version_available() -> Option<String> {
    let latest = latest_version(&fetch_appcast()?)?;
    version_gt(&latest, &current_version()).then_some(latest)
}

#[cfg(test)]
mod tests {
    use super::{latest_version, version_gt};

    #[test]
    fn version_comparison() {
        assert!(version_gt("0.1.3", "0.1.2"));
        assert!(version_gt("0.2.0", "0.1.9"));
        assert!(version_gt("1.0.0", "0.9.9"));
        assert!(!version_gt("0.1.2", "0.1.2"));
        assert!(!version_gt("0.1.2", "0.1.3"));
        assert!(version_gt("0.1.1", "0.1"));
        assert!(!version_gt("0.1", "0.1.0"));
    }

    #[test]
    fn parses_newest_version() {
        let xml = "<item><sparkle:shortVersionString>0.1.3</sparkle:shortVersionString>\
                   </item><item><sparkle:shortVersionString>0.1.2</sparkle:shortVersionString></item>";
        assert_eq!(latest_version(xml).as_deref(), Some("0.1.3"));
        assert_eq!(latest_version("<x/>"), None);
    }
}
