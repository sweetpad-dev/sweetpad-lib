use std::path::PathBuf;
use std::process::Command;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_sweetpad")
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn xcconfig_fixture(name: &str) -> PathBuf {
    fixtures_root().join(format!(
        "_synthetic-xcconfigs/xcode-26.5.0/xcconfigs/{name}.xcconfig"
    ))
}

#[test]
fn parse_pbxproj_succeeds_on_synthetic_scratch() {
    let path = fixtures_root()
        .join("_synthetic-xcconfigs/xcode-26.5.0/project/Scratch.xcodeproj/project.pbxproj");
    let out = Command::new(binary())
        .arg("parse-pbxproj")
        .arg(&path)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("PBXProject"));
}

#[test]
fn parse_xcconfig_succeeds() {
    let out = Command::new(binary())
        .arg("parse-xcconfig")
        .arg(xcconfig_fixture("conditional-sdk"))
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("FOO"));
    assert!(stdout.contains("Condition"));
}

#[test]
fn parse_xcscheme_succeeds() {
    let path = fixtures_root().join(
        "kingfisher/xcode-26.5.0/raw/Kingfisher.xcodeproj/xcshareddata/xcschemes/Kingfisher.xcscheme",
    );
    let out = Command::new(binary())
        .arg("parse-xcscheme")
        .arg(&path)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Scheme"));
    assert!(stdout.contains("BuildAction"));
}

#[test]
fn resolve_conditional_sdk_macos() {
    let out = Command::new(binary())
        .arg("resolve")
        .arg(xcconfig_fixture("conditional-sdk"))
        .arg("--sdk")
        .arg("macosx")
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.lines().any(|l| l == "FOO=macos"),
        "expected `FOO=macos` line; got:\n{stdout}"
    );
}

#[test]
fn resolve_conditional_sdk_iphoneos() {
    let out = Command::new(binary())
        .arg("resolve")
        .arg(xcconfig_fixture("conditional-sdk"))
        .arg("--sdk")
        .arg("iphoneos")
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.lines().any(|l| l == "FOO=ios_device"),
        "expected `FOO=ios_device` line; got:\n{stdout}"
    );
}

#[test]
fn resolve_include_brings_in_other_xcconfig() {
    let out = Command::new(binary())
        .arg("resolve")
        .arg(xcconfig_fixture("include-directive"))
        .arg("--sdk")
        .arg("macosx")
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.lines().any(|l| l == "FOO=macos"));
    assert!(stdout.lines().any(|l| l == "EXTRA=layered"));
}

#[test]
fn list_command_on_scratch_project() {
    let path = fixtures_root().join("_synthetic-xcconfigs/xcode-26.5.0/project/Scratch.xcodeproj");
    let out = Command::new(binary())
        .arg("list")
        .arg("--project")
        .arg(&path)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Information about project \"Scratch\""));
    assert!(stdout.contains("Targets:"));
    assert!(stdout.contains("Scratch"));
    assert!(stdout.contains("Build Configurations:"));
    assert!(stdout.contains("Debug"));
    assert!(stdout.contains("Release"));
}

#[test]
fn list_command_shows_icecubes_schemes() {
    let path = fixtures_root().join("ice-cubes/xcode-26.5.0/raw/IceCubesApp.xcodeproj");
    let out = Command::new(binary())
        .arg("list")
        .arg("--project")
        .arg(&path)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Schemes:"));
    for expected in [
        "IceCubesActionExtension",
        "IceCubesApp",
        "IceCubesAppWidgetsExtensionExtension",
        "IceCubesNotifications",
        "IceCubesShareExtension",
    ] {
        assert!(
            stdout.contains(expected),
            "expected scheme '{expected}' in output:\n{stdout}"
        );
    }
}

#[test]
fn build_settings_scratch_debug() {
    // No --xcspec-root: exercises the catalog baked into the binary, so Apple's
    // defaults are layered under the project's settings out of the box.
    let path = fixtures_root().join("_synthetic-xcconfigs/xcode-26.5.0/project/Scratch.xcodeproj");
    let out = Command::new(binary())
        .arg("build-settings")
        .arg("--project")
        .arg(&path)
        .arg("--target")
        .arg("Scratch")
        .arg("--config")
        .arg("Debug")
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Build settings for action build and target Scratch:"));
    for expected in [
        "    ALWAYS_SEARCH_USER_PATHS = NO",
        "    MACOSX_DEPLOYMENT_TARGET = 12.0",
        "    SWIFT_VERSION = 5.0",
        "    PRODUCT_NAME = Scratch",
    ] {
        assert!(
            stdout.contains(expected),
            "missing `{expected}` in output:\n{stdout}"
        );
    }
    // With the defaults catalog present, the canonical `macosx` SDKROOT
    // resolves to the absolute SDK path (matching real xcodebuild / the
    // explicit --xcspec-root path).
    assert!(
        stdout
            .lines()
            .any(|l| l.starts_with("    SDKROOT = ") && l.trim_end().ends_with("MacOSX.sdk")),
        "expected SDKROOT to resolve to a MacOSX.sdk path:\n{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn build_settings_via_xcode_arg() {
    use std::os::unix::fs::symlink;

    // Assemble a minimal Xcode.app skeleton that symlinks the corpus cache into
    // the real Contents/{SharedFrameworks,Developer/Platforms} layout, so
    // `--xcode` discovers the same specs the explicit roots would.
    let cache = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("xcspec-cache/xcode-26.5.0");
    let tmp = std::env::temp_dir().join(format!("sweetpad-xcode-cli-{}", std::process::id()));
    let contents = tmp.join("Xcode.app/Contents");
    std::fs::create_dir_all(contents.join("Developer")).unwrap();
    symlink(
        cache.join("SharedFrameworks"),
        contents.join("SharedFrameworks"),
    )
    .unwrap();
    symlink(
        cache.join("sdksettings/Platforms"),
        contents.join("Developer/Platforms"),
    )
    .unwrap();
    std::fs::write(
        contents.join("version.plist"),
        "<plist><dict><key>CFBundleShortVersionString</key><string>26.5</string>\
         <key>ProductBuildVersion</key><string>17F6</string></dict></plist>",
    )
    .unwrap();

    let path = fixtures_root().join("_synthetic-xcconfigs/xcode-26.5.0/project/Scratch.xcodeproj");
    let out = Command::new(binary())
        .arg("build-settings")
        .arg("--project")
        .arg(&path)
        .arg("--target")
        .arg("Scratch")
        .arg("--config")
        .arg("Debug")
        .arg("--xcode")
        .arg(tmp.join("Xcode.app"))
        .output()
        .expect("spawn");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // DEVELOPER_DIR + version come from the install we pointed --xcode at, not
    // whatever Xcode is xcode-select'ed.
    assert!(
        stdout
            .lines()
            .any(|l| l.starts_with("    DEVELOPER_DIR = ")
                && l.contains("Xcode.app/Contents/Developer")),
        "DEVELOPER_DIR not sourced from --xcode:\n{stdout}"
    );
    assert!(stdout.contains("    XCODE_VERSION_MAJOR = 26"));
    // SDKROOT resolves to a real SDK path discovered under that install.
    assert!(
        stdout
            .lines()
            .any(|l| l.starts_with("    SDKROOT = ") && l.trim_end().ends_with("MacOSX.sdk")),
        "SDKROOT not discovered under --xcode:\n{stdout}"
    );
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn build_settings_layers_extra_xcconfig_for_macos() {
    let path = fixtures_root().join("_synthetic-xcconfigs/xcode-26.5.0/project/Scratch.xcodeproj");
    let xcconfig = xcconfig_fixture("conditional-sdk");
    let out = Command::new(binary())
        .arg("build-settings")
        .arg("--project")
        .arg(&path)
        .arg("--target")
        .arg("Scratch")
        .arg("--config")
        .arg("Debug")
        .arg("--sdk")
        .arg("macosx")
        .arg("--xcconfig")
        .arg(&xcconfig)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.lines().any(|l| l == "    FOO = macos"),
        "expected `    FOO = macos` in output:\n{stdout}"
    );
    assert!(stdout.contains("    SWIFT_VERSION = 5.0"));
}

#[test]
fn build_settings_layers_extra_xcconfig_for_iphoneos() {
    let path = fixtures_root().join("_synthetic-xcconfigs/xcode-26.5.0/project/Scratch.xcodeproj");
    let xcconfig = xcconfig_fixture("conditional-sdk");
    let out = Command::new(binary())
        .arg("build-settings")
        .arg("--project")
        .arg(&path)
        .arg("--target")
        .arg("Scratch")
        .arg("--config")
        .arg("Debug")
        .arg("--sdk")
        .arg("iphoneos")
        .arg("--xcconfig")
        .arg(&xcconfig)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.lines().any(|l| l == "    FOO = ios_device"),
        "expected `    FOO = ios_device` in output:\n{stdout}"
    );
}

#[test]
fn build_settings_unknown_target_errors() {
    let path = fixtures_root().join("_synthetic-xcconfigs/xcode-26.5.0/project/Scratch.xcodeproj");
    let out = Command::new(binary())
        .arg("build-settings")
        .arg("--project")
        .arg(&path)
        .arg("--target")
        .arg("Nonexistent")
        .arg("--config")
        .arg("Debug")
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no target named"));
}

#[test]
fn missing_path_yields_failure() {
    let out = Command::new(binary())
        .arg("parse-pbxproj")
        .arg("/nonexistent/path/project.pbxproj")
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("error:"));
}

#[test]
fn no_subcommand_shows_help_and_fails() {
    let out = Command::new(binary()).output().expect("spawn");
    assert!(!out.status.success());
}

fn kingfisher_proj() -> PathBuf {
    fixtures_root().join("kingfisher/xcode-26.5.0/raw/Kingfisher.xcodeproj")
}

fn build_settings(extra: &[&str]) -> std::process::Output {
    Command::new(binary())
        .arg("build-settings")
        .arg("--project")
        .arg(kingfisher_proj())
        .args(["--target", "Kingfisher", "--config", "Debug", "--json"])
        .args(extra)
        .output()
        .expect("spawn")
}

#[test]
fn build_settings_destination_collapses_macos_archs() {
    // No destination on macOS reports the SDK's full standard arch list,
    // but a bound macOS destination collapses ARCHS to the active arch —
    // the headline destination-aware behaviour.
    let no_dest = build_settings(&["--sdk", "macosx"]);
    assert!(no_dest.status.success());
    let no_dest = String::from_utf8_lossy(&no_dest.stdout);
    assert!(
        no_dest.contains("\"ARCHS\": \"arm64 x86_64\""),
        "expected full arch list without destination: {no_dest}"
    );

    let dest = build_settings(&["--destination", "platform=macOS"]);
    assert!(dest.status.success());
    let dest = String::from_utf8_lossy(&dest.stdout);
    assert!(
        dest.contains("\"ARCHS\": \"arm64\""),
        "expected collapsed arch with destination: {dest}"
    );
}

#[test]
fn build_settings_destination_supplies_platform() {
    // An `id=`-only simulator destination (the common IDE case) supplies the
    // SDK with no `--sdk`, and still resolves the catalog-backed product keys.
    let out = build_settings(&["--destination", "platform=iOS Simulator,id=ABC-123"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"PLATFORM_NAME\": \"iphonesimulator\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"WRAPPER_NAME\": \"Kingfisher.framework\""),
        "{stdout}"
    );
}

#[test]
fn build_settings_rejects_invalid_destination() {
    let out = build_settings(&["--destination", "platform=Android"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("invalid --destination"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
