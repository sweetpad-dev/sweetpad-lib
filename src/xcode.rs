//! What Xcode installation is active right now.
//!
//! The library shells out the same way `xcrun xcodebuild -version` and
//! `xcode-select -p` would, then reads `version.plist` next to the active
//! Developer directory. Caching is the caller's problem — these functions
//! always re-detect.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Snapshot of the active Xcode toolchain.
#[derive(Debug, Clone)]
pub struct ActiveInstall {
    /// Absolute path to the `Developer` directory (what `xcode-select -p`
    /// prints; what `DEVELOPER_DIR` env var overrides).
    pub developer_dir: PathBuf,
    /// `CFBundleShortVersionString` from `version.plist` (e.g. `26.0.1`).
    /// Empty when the plist can't be read.
    pub short_version: String,
    /// `ProductBuildVersion` from `version.plist` (e.g. `17A400`).
    /// Empty when the plist can't be read.
    pub build_version: String,
}

impl ActiveInstall {
    /// Parsed major version (`26` for Xcode 26.0.1). Returns 0 when
    /// [`Self::short_version`] is empty or unparseable.
    #[must_use]
    pub fn major_version(&self) -> u32 {
        self.short_version
            .split('.')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    /// Combined `<short>-<build>` string. xcodebuild's
    /// `-showBuildSettings` reports `XCODE_PRODUCT_BUILD_VERSION` in this
    /// form. Returns `"Unknown"` when either component is missing —
    /// mirroring xcodebuild's own fallback.
    #[must_use]
    pub fn product_build_version(&self) -> String {
        if self.short_version.is_empty() || self.build_version.is_empty() {
            "Unknown".into()
        } else {
            format!("{}-{}", self.short_version, self.build_version)
        }
    }
}

/// Detect the active Xcode by honouring `DEVELOPER_DIR`, then
/// `xcode-select -p`, then a hard-coded fallback. Reads `version.plist`
/// for short + build version when available.
#[must_use]
pub fn active_install() -> ActiveInstall {
    let developer_dir = detect_developer_dir();
    let (short_version, build_version) = read_version_plist(&developer_dir);
    ActiveInstall {
        developer_dir,
        short_version,
        build_version,
    }
}

/// Just the active Developer directory — `DEVELOPER_DIR` if set, else
/// `xcode-select -p`, else the standard `/Applications/Xcode.app` path.
#[must_use]
pub fn detect_developer_dir() -> PathBuf {
    if let Ok(val) = std::env::var("DEVELOPER_DIR")
        && !val.is_empty()
    {
        return PathBuf::from(val);
    }
    if let Ok(output) = Command::new("xcode-select").arg("-p").output()
        && output.status.success()
    {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return PathBuf::from(s);
        }
    }
    PathBuf::from("/Applications/Xcode.app/Contents/Developer")
}

fn read_version_plist(developer_dir: &Path) -> (String, String) {
    let plist_path = developer_dir.parent().map(|p| p.join("version.plist"));
    let Some(path) = plist_path else {
        return (String::new(), String::new());
    };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return (String::new(), String::new());
    };
    (
        extract_plist_string(&contents, "CFBundleShortVersionString").unwrap_or_default(),
        extract_plist_string(&contents, "ProductBuildVersion").unwrap_or_default(),
    )
}

/// Cheap XML scrape — version.plist always emits `<key>K</key><string>V</string>`
/// once per key in our captured corpus.
fn extract_plist_string(xml: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let start = xml.find(&needle)?;
    let after = &xml[start + needle.len()..];
    let open = after.find("<string>")?;
    let close = after.find("</string>")?;
    if close <= open + "<string>".len() {
        return None;
    }
    Some(after[open + "<string>".len()..close].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_version_parses_from_short_version() {
        let i = ActiveInstall {
            developer_dir: PathBuf::from("/tmp"),
            short_version: "26.0.1".into(),
            build_version: "17A400".into(),
        };
        assert_eq!(i.major_version(), 26);
        assert_eq!(i.product_build_version(), "26.0.1-17A400");
    }

    #[test]
    fn product_build_version_falls_back_to_unknown() {
        let i = ActiveInstall {
            developer_dir: PathBuf::from("/tmp"),
            short_version: String::new(),
            build_version: String::new(),
        };
        assert_eq!(i.product_build_version(), "Unknown");
        assert_eq!(i.major_version(), 0);
    }

    #[test]
    fn extracts_plist_string_value() {
        let xml = r"<plist>
            <dict>
              <key>CFBundleShortVersionString</key>
              <string>26.0.1</string>
              <key>ProductBuildVersion</key>
              <string>17A400</string>
            </dict>
          </plist>";
        assert_eq!(
            extract_plist_string(xml, "CFBundleShortVersionString").as_deref(),
            Some("26.0.1"),
        );
        assert_eq!(
            extract_plist_string(xml, "ProductBuildVersion").as_deref(),
            Some("17A400"),
        );
        assert!(extract_plist_string(xml, "Missing").is_none());
    }
}
