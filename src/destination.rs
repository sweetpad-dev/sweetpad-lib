//! Run-destination metadata: the device/platform/OS combo a build is
//! targeted at.
//!
//! `xcodebuild -showBuildSettings -destination "platform=…,name=…,OS=…"`
//! emits many settings that depend on the run destination (`ARCHS` reduces
//! to a single active arch, `ONLY_ACTIVE_ARCH` flips to `YES`, the
//! `__IS_NOT_SIMULATOR` internal flag toggles, etc.). The captured oracles
//! encode the destination in the filename suffix; we parse that here so
//! [`crate::project::built_in_settings`] can synthesize destination-aware
//! defaults.

/// Where a build is targeted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDestination {
    /// Canonical SDK platform name: `macosx`, `iphonesimulator`,
    /// `iphoneos`, `appletvsimulator`, `appletvos`, `watchsimulator`,
    /// `watchos`, `xrsimulator`, `xros`.
    pub platform: String,
    /// OS version reported by the destination (e.g. `26.0.1`). Empty for
    /// macOS, where the captured oracles don't include a version segment.
    pub os_version: String,
    /// Human-readable device label drawn from the oracle filename
    /// (e.g. `iPad-A16`, `Apple-Vision-Pro`). Used for
    /// `RUN_DESTINATION_DEVICE_NAME`-like settings.
    pub device_name: String,
    /// Architecture the destination actually executes — `arm64` on every
    /// captured device, `x86_64` only on older Intel macs (we never see
    /// that in the corpus).
    pub arch: String,
}

impl RunDestination {
    /// True for any `*-simulator` platform.
    #[must_use]
    pub fn is_simulator(&self) -> bool {
        self.platform.ends_with("simulator")
    }

    /// True when the destination is macOS (host, not Catalyst).
    #[must_use]
    pub fn is_macos(&self) -> bool {
        self.platform == "macosx"
    }
}

/// Parse the destination suffix that appears in oracle filenames after
/// the `Config__` prefix. Accepts:
///
/// ```text
/// macOS                                               → macosx
/// iOS-Simulator_OS26.0.1_iPad-A16                     → iphonesimulator + OS + device
/// iOS-Simulator_OS18.1_iPad-10th-generation           → idem
/// tvOS-Simulator_OS26.0_Apple-TV                      → appletvsimulator
/// watchOS-Simulator_OS26.0_Apple-Watch-SE-3-40mm      → watchsimulator
/// visionOS-Simulator_OS26.0_Apple-Vision-Pro          → xrsimulator
/// ```
///
/// Returns `None` for shapes we don't recognise.
#[must_use]
pub fn parse_destination_suffix(s: &str) -> Option<RunDestination> {
    // The macOS case has no OS / device segments.
    if s == "macOS" || s == "macos" {
        return Some(RunDestination {
            platform: "macosx".into(),
            os_version: String::new(),
            device_name: String::new(),
            arch: "arm64".into(),
        });
    }
    // The remaining shapes are `<PlatformLabel>_OS<version>_<device>`.
    let mut parts = s.splitn(3, '_');
    let platform_label = parts.next()?;
    let platform = platform_for_label(platform_label)?;
    let os_version = parts.next().and_then(|p| p.strip_prefix("OS"))?;
    let device_name = parts.next()?;
    Some(RunDestination {
        platform,
        os_version: os_version.into(),
        device_name: device_name.into(),
        arch: "arm64".into(),
    })
}

fn platform_for_label(label: &str) -> Option<String> {
    Some(
        match label {
            "iOS-Simulator" => "iphonesimulator",
            "iOS" => "iphoneos",
            "tvOS-Simulator" => "appletvsimulator",
            "tvOS" => "appletvos",
            "watchOS-Simulator" => "watchsimulator",
            "watchOS" => "watchos",
            "visionOS-Simulator" => "xrsimulator",
            "visionOS" => "xros",
            "macOS" | "macos" => "macosx",
            _ => return None,
        }
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_macos() {
        let d = parse_destination_suffix("macOS").unwrap();
        assert_eq!(d.platform, "macosx");
        assert!(d.os_version.is_empty());
        assert!(d.is_macos());
        assert!(!d.is_simulator());
    }

    #[test]
    fn parses_ios_simulator() {
        let d = parse_destination_suffix("iOS-Simulator_OS26.0.1_iPad-A16").unwrap();
        assert_eq!(d.platform, "iphonesimulator");
        assert_eq!(d.os_version, "26.0.1");
        assert_eq!(d.device_name, "iPad-A16");
        assert_eq!(d.arch, "arm64");
        assert!(d.is_simulator());
    }

    #[test]
    fn parses_visionos_simulator() {
        let d = parse_destination_suffix("visionOS-Simulator_OS26.0_Apple-Vision-Pro").unwrap();
        assert_eq!(d.platform, "xrsimulator");
        assert_eq!(d.os_version, "26.0");
        assert_eq!(d.device_name, "Apple-Vision-Pro");
        assert!(d.is_simulator());
    }

    #[test]
    fn parses_watchos_simulator_with_device_hyphens() {
        let d = parse_destination_suffix("watchOS-Simulator_OS26.0_Apple-Watch-SE-3-40mm").unwrap();
        assert_eq!(d.platform, "watchsimulator");
        assert_eq!(d.device_name, "Apple-Watch-SE-3-40mm");
    }

    #[test]
    fn parses_tvos_simulator() {
        let d = parse_destination_suffix("tvOS-Simulator_OS26.0_Apple-TV").unwrap();
        assert_eq!(d.platform, "appletvsimulator");
    }

    #[test]
    fn rejects_unknown_shapes() {
        assert!(parse_destination_suffix("totally-bogus").is_none());
        assert!(parse_destination_suffix("iOS-Simulator").is_none()); // missing OS
        assert!(parse_destination_suffix("iOS-Simulator_OS26.0").is_none()); // missing device
    }
}
