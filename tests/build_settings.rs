//! Library-level coverage of the build-settings orchestration
//! (`build_settings::resolve_build_settings`) — the behaviours previously
//! exercised end-to-end through the (removed) CLI `build-settings` command.

use std::collections::BTreeMap;
use std::path::PathBuf;

use sweetpad::build_settings::{BuildSettingsOptions, resolve_build_settings};
use sweetpad::destination::parse_destination_arg;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn scratch_proj() -> PathBuf {
    fixtures_root().join("_synthetic-xcconfigs/xcode-26.5.0/project/Scratch.xcodeproj")
}

fn kingfisher_proj() -> PathBuf {
    fixtures_root().join("kingfisher/xcode-26.5.0/raw/Kingfisher.xcodeproj")
}

fn xcconfig_fixture(name: &str) -> PathBuf {
    fixtures_root().join(format!(
        "_synthetic-xcconfigs/xcode-26.5.0/xcconfigs/{name}.xcconfig"
    ))
}

/// Resolve a single target and return its settings map.
fn resolve_one(opts: BuildSettingsOptions) -> BTreeMap<String, String> {
    let mut out = resolve_build_settings(&opts).unwrap();
    assert_eq!(out.len(), 1, "expected exactly one resolved target");
    out.remove(0).settings
}

fn scratch_opts() -> BuildSettingsOptions {
    BuildSettingsOptions {
        project: Some(scratch_proj()),
        target: Some("Scratch".to_string()),
        configuration: "Debug".to_string(),
        sdk: "macosx".to_string(),
        arch: "arm64".to_string(),
        ..Default::default()
    }
}

fn kingfisher_opts() -> BuildSettingsOptions {
    BuildSettingsOptions {
        project: Some(kingfisher_proj()),
        target: Some("Kingfisher".to_string()),
        configuration: "Debug".to_string(),
        sdk: "macosx".to_string(),
        arch: "arm64".to_string(),
        ..Default::default()
    }
}

#[test]
fn scratch_debug_uses_embedded_catalog_defaults() {
    // No xcode/xcspec_root: exercises the catalog baked into the binary, so
    // Apple's defaults layer under the project's settings out of the box.
    let s = resolve_one(scratch_opts());
    assert_eq!(
        s.get("ALWAYS_SEARCH_USER_PATHS").map(String::as_str),
        Some("NO")
    );
    assert_eq!(
        s.get("MACOSX_DEPLOYMENT_TARGET").map(String::as_str),
        Some("12.0")
    );
    assert_eq!(s.get("SWIFT_VERSION").map(String::as_str), Some("5.0"));
    assert_eq!(s.get("PRODUCT_NAME").map(String::as_str), Some("Scratch"));
    // The canonical `macosx` SDKROOT resolves to the absolute SDK path.
    let sdkroot = s.get("SDKROOT").expect("SDKROOT present");
    assert!(sdkroot.ends_with("MacOSX.sdk"), "SDKROOT = {sdkroot}");
}

#[test]
fn layers_extra_xcconfig_macos() {
    let opts = BuildSettingsOptions {
        xcconfig: Some(xcconfig_fixture("conditional-sdk")),
        ..scratch_opts()
    };
    let s = resolve_one(opts);
    assert_eq!(s.get("FOO").map(String::as_str), Some("macos"));
    assert_eq!(s.get("SWIFT_VERSION").map(String::as_str), Some("5.0"));
}

#[test]
fn layers_extra_xcconfig_iphoneos() {
    let opts = BuildSettingsOptions {
        sdk: "iphoneos".to_string(),
        xcconfig: Some(xcconfig_fixture("conditional-sdk")),
        ..scratch_opts()
    };
    let s = resolve_one(opts);
    assert_eq!(s.get("FOO").map(String::as_str), Some("ios_device"));
}

#[test]
fn unknown_target_errors() {
    let opts = BuildSettingsOptions {
        target: Some("Nonexistent".to_string()),
        ..scratch_opts()
    };
    let err = resolve_build_settings(&opts).unwrap_err();
    assert!(err.contains("no target named"), "err: {err}");
}

#[test]
fn destination_collapses_macos_archs() {
    // No destination on macOS reports the SDK's full standard arch list; a
    // bound macOS destination collapses ARCHS to the active arch — the
    // headline destination-aware behaviour.
    let no_dest = resolve_one(kingfisher_opts());
    assert_eq!(
        no_dest.get("ARCHS").map(String::as_str),
        Some("arm64 x86_64")
    );

    let opts = BuildSettingsOptions {
        destination: parse_destination_arg("platform=macOS"),
        ..kingfisher_opts()
    };
    let dest = resolve_one(opts);
    assert_eq!(dest.get("ARCHS").map(String::as_str), Some("arm64"));
}

#[test]
fn destination_supplies_platform() {
    // An `id=`-only simulator destination (the common IDE case) supplies the
    // SDK with no explicit `sdk`, and still resolves catalog-backed keys.
    let opts = BuildSettingsOptions {
        destination: parse_destination_arg("platform=iOS Simulator,id=ABC-123"),
        ..kingfisher_opts()
    };
    assert!(opts.destination.is_some(), "destination should parse");
    let s = resolve_one(opts);
    assert_eq!(
        s.get("PLATFORM_NAME").map(String::as_str),
        Some("iphonesimulator")
    );
    assert_eq!(
        s.get("WRAPPER_NAME").map(String::as_str),
        Some("Kingfisher.framework")
    );
}

#[test]
fn invalid_destination_is_rejected_at_parse() {
    // Each caller parses the destination string; an unknown platform yields
    // `None` (the CLI surfaced this as "invalid --destination").
    assert!(parse_destination_arg("platform=Android").is_none());
}
