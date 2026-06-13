//! API-key resolution and storage.
//!
//! Resolution order: `ANTHROPIC_API_KEY` environment variable, then the macOS
//! Keychain (service `daisynotes-anthropic`) via the `security` CLI. Lookup happens
//! per request inside the worker, so a key added after launch is picked up
//! without restarting the app.

use std::process::Command;

/// Keychain service name under which Muse stores the Anthropic key.
const KEYCHAIN_SERVICE: &str = "daisynotes-anthropic";

/// Keychain account name used by [`store_api_key`].
const KEYCHAIN_ACCOUNT: &str = "muse";

/// Resolves the Anthropic API key.
///
/// Checks the `ANTHROPIC_API_KEY` environment variable first, then the macOS
/// Keychain (`security find-generic-password -s daisynotes-anthropic -w`). Output
/// is trimmed; empty values are treated as absent.
pub fn resolve_api_key() -> Option<String> {
    if let Ok(value) = std::env::var("ANTHROPIC_API_KEY")
        && let Some(key) = non_empty_trimmed(&value)
    {
        return Some(key);
    }
    keychain_lookup()
}

/// Stores `key` in the macOS Keychain under the `daisynotes-anthropic` service
/// (`security add-generic-password -U -s daisynotes-anthropic -a muse -w <key>`),
/// updating any existing entry. Returns `true` on success.
pub fn store_api_key(key: &str) -> bool {
    let stored = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            KEYCHAIN_ACCOUNT,
            "-w",
            key,
        ])
        .output()
        .is_ok_and(|output| output.status.success());
    if !stored {
        tracing::warn!("daisynotes-api: failed to store key in the keychain");
    }
    stored
}

fn keychain_lookup() -> Option<String> {
    let output = Command::new("security")
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    non_empty_trimmed(&String::from_utf8_lossy(&output.stdout))
}

fn non_empty_trimmed(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming_rejects_empty_and_whitespace_values() {
        assert_eq!(non_empty_trimmed(""), None);
        assert_eq!(non_empty_trimmed("   \n"), None);
        assert_eq!(
            non_empty_trimmed("  sk-ant-test\n"),
            Some("sk-ant-test".to_string())
        );
    }
}
