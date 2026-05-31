//! N-API bindings: the resolver exposed to the sweetpad VS Code extension as a
//! native node addon (`.node`), so the extension calls into Rust in-process
//! instead of spawning the CLI. Built into the cdylib under `--features node`
//! via `@napi-rs/cli` (`napi build`), which also generates the `.d.ts`. The CLI
//! (`main.rs`) stays the standalone / test entry point.
//!
//! Each function returns a typed object (`#[napi(object)]`) mapped from the
//! library's own structs — no JSON round-tripping.

// N-API entry points must take owned args (the runtime marshals them in); a
// borrowed `&str` isn't an option at the boundary.
#![allow(clippy::needless_pass_by_value)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use napi_derive::napi;

use crate::destination::parse_destination_arg;
use crate::{project, workspace, xcode};

/// Active Xcode toolchain info. Mirrors `xcrun xcodebuild -version` plus the
/// resolved `DEVELOPER_DIR`.
#[napi(object)]
pub struct XcodeVersion {
    pub developer_dir: String,
    pub short_version: String,
    pub build_version: String,
    pub major_version: u32,
}

/// Resolve the active Xcode (the one `xcode-select` points at).
#[napi]
#[must_use]
pub fn xcode_version() -> XcodeVersion {
    let info = xcode::active_install();
    XcodeVersion {
        developer_dir: info.developer_dir.display().to_string(),
        short_version: info.short_version.clone(),
        build_version: info.build_version.clone(),
        major_version: info.major_version(),
    }
}

/// A single `.xcodeproj`'s targets, configurations, and shared schemes.
/// Mirrors `xcodebuild -list -project`.
#[napi(object)]
pub struct ProjectInfo {
    pub name: String,
    pub targets: Vec<String>,
    pub configurations: Vec<String>,
    pub schemes: Vec<String>,
}

/// List a `.xcodeproj`'s targets, configurations, and shared schemes.
#[napi]
pub fn list_project(path: String) -> napi::Result<ProjectInfo> {
    let project = project::open(Path::new(&path)).map_err(to_napi_err)?;
    Ok(ProjectInfo {
        name: project.name,
        targets: project.targets.into_iter().map(|t| t.name).collect(),
        configurations: project.configurations,
        schemes: project.schemes,
    })
}

/// A `.xcworkspace`'s declared `.xcodeproj` paths and merged shared schemes.
/// Mirrors `xcodebuild -list -workspace`, plus the `projects` paths.
#[napi(object)]
pub struct WorkspaceInfo {
    pub name: String,
    /// Absolute paths of every `.xcodeproj` the workspace declares, in order.
    pub projects: Vec<String>,
    pub schemes: Vec<String>,
}

/// List a `.xcworkspace`'s member projects and merged shared schemes.
#[napi]
pub fn list_workspace(path: String) -> napi::Result<WorkspaceInfo> {
    let ws = workspace::open(Path::new(&path)).map_err(to_napi_err)?;
    let projects = ws
        .project_refs
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    let schemes = ws.merged_schemes();
    Ok(WorkspaceInfo {
        name: ws.name,
        projects,
        schemes,
    })
}

/// Options for a `buildSettings` resolution — mirrors
/// `xcodebuild -showBuildSettings`. Either `project` or `workspace` is required;
/// either `scheme` or `target` selects what to resolve.
#[napi(object)]
pub struct BuildSettingsOptions {
    pub project: Option<String>,
    pub workspace: Option<String>,
    pub scheme: Option<String>,
    pub target: Option<String>,
    pub configuration: String,
    /// SDK to bind conditionals to. Defaults to `macosx`. Ignored when
    /// `destination` is set (the destination's platform wins).
    pub sdk: Option<String>,
    /// Arch to bind conditionals to. Defaults to `arm64`. Ignored when
    /// `destination` is set.
    pub arch: Option<String>,
    /// `xcodebuild -destination` string, e.g. `platform=iOS Simulator,id=…`.
    pub destination: Option<String>,
    /// Extra `.xcconfig` overlay (`xcodebuild -xcconfig`).
    pub xcconfig: Option<String>,
    /// A specific `Xcode.app` / `Contents/Developer` to resolve against.
    pub xcode: Option<String>,
    /// `xcodebuild -derivedDataPath` override.
    pub derived_data_path: Option<String>,
}

/// One target's resolved build settings (`{ KEY: VALUE }`).
#[napi(object)]
pub struct TargetBuildSettings {
    pub target: String,
    pub settings: HashMap<String, String>,
}

/// Resolve build settings for a scheme or target across a project/workspace.
/// Mirrors `xcodebuild -showBuildSettings`.
#[napi]
pub fn build_settings(options: BuildSettingsOptions) -> napi::Result<Vec<TargetBuildSettings>> {
    let destination = match options.destination.as_deref() {
        Some(s) => Some(
            parse_destination_arg(s)
                .ok_or_else(|| napi::Error::from_reason(format!("invalid destination: {s:?}")))?,
        ),
        None => None,
    };
    let opts = crate::build_settings::BuildSettingsOptions {
        project: options.project.map(PathBuf::from),
        workspace: options.workspace.map(PathBuf::from),
        scheme: options.scheme,
        target: options.target,
        configuration: options.configuration,
        sdk: options.sdk.unwrap_or_else(|| "macosx".into()),
        arch: options.arch.unwrap_or_else(|| "arm64".into()),
        destination,
        xcconfig: options.xcconfig.map(PathBuf::from),
        xcode: options.xcode.map(PathBuf::from),
        xcspec_root: None,
        sdksettings_root: None,
        catalog_cache: None,
        derived_data_path: options.derived_data_path.map(PathBuf::from),
    };
    let resolved = crate::build_settings::resolve_build_settings(&opts).map_err(to_napi_err)?;
    Ok(resolved
        .into_iter()
        .map(|t| TargetBuildSettings {
            target: t.target,
            settings: t.settings.into_iter().collect(),
        })
        .collect())
}

fn to_napi_err(e: impl std::fmt::Display) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}
