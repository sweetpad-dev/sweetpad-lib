//! Typed model of an `.xcworkspace`.
//!
//! Reads `contents.xcworkspacedata` (XML, same parser as `.xcscheme`) and
//! the workspace-level shared schemes under `xcshareddata/xcschemes/`.
//! Returns absolute paths to every referenced `.xcodeproj`.
//!
//! What `contents.xcworkspacedata` looks like:
//!
//! ```xml
//! <Workspace version="1.0">
//!   <FileRef location="group:Foo.xcodeproj"/>
//!   <FileRef location="group:Sub/Bar.xcodeproj"/>
//!   <Group location="container:..." name="Subgroup">
//!     <FileRef location="group:Baz.xcodeproj"/>
//!   </Group>
//! </Workspace>
//! ```
//!
//! Location prefixes we handle: `container:` (workspace dir), `group:`
//! (parent-group dir, falling back to workspace dir), `absolute:` (absolute
//! path), `self:` (workspace dir), `developer:` (`DEVELOPER_DIR`).

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::xcode;
use crate::xcscheme::{self, Element};

#[derive(Debug, Clone)]
pub struct Workspace {
    /// `.xcworkspace` basename without extension (e.g. `Kingfisher`).
    pub name: String,
    /// Absolute path to the `.xcworkspace` directory.
    pub path: PathBuf,
    /// Absolute paths to every `.xcodeproj` referenced by the workspace,
    /// in declaration order.
    pub project_refs: Vec<PathBuf>,
    /// Shared scheme names from `<ws>/xcshareddata/xcschemes/`, sorted
    /// alphabetically. Matches the order `xcodebuild -list -workspace`
    /// reports.
    pub schemes: Vec<String>,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Parse(xcscheme::Error),
    BadWorkspace(String),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<xcscheme::Error> for Error {
    fn from(e: xcscheme::Error) -> Self {
        Error::Parse(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Parse(e) => write!(f, "{e}"),
            Error::BadWorkspace(s) => write!(f, "invalid workspace: {s}"),
        }
    }
}

impl std::error::Error for Error {}

/// Open a `.xcworkspace` directory and extract referenced projects + schemes.
pub fn open(workspace_path: &Path) -> Result<Workspace, Error> {
    let contents = workspace_path.join("contents.xcworkspacedata");
    let root = xcscheme::parse_file(&contents)?;
    if root.name != "Workspace" {
        return Err(Error::BadWorkspace(format!(
            "expected root <Workspace>, got <{}>",
            root.name
        )));
    }

    // `group:` / `container:` references are anchored at the directory
    // *containing* the `.xcworkspace`, not the workspace bundle itself:
    // `Foo.xcworkspace/contents.xcworkspacedata` says `group:Foo.xcodeproj`
    // and the project lives next to (not inside) the workspace.
    let base = workspace_path
        .parent()
        .map_or_else(|| workspace_path.to_path_buf(), Path::to_path_buf);
    let mut project_refs = Vec::new();
    collect_project_refs(&root, &base, &mut project_refs);

    let schemes = scan_shared_schemes(workspace_path);

    let name = workspace_path
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_string();

    Ok(Workspace {
        name,
        path: workspace_path.to_path_buf(),
        project_refs,
        schemes,
    })
}

impl Workspace {
    /// Schemes that `xcodebuild -list -workspace` would surface: the
    /// workspace's own shared schemes UNION every member project's shared
    /// schemes, deduplicated and sorted. Opens each project to scan its
    /// `xcshareddata/xcschemes/`; failures (missing project, unreadable
    /// directory) are skipped silently.
    #[must_use]
    pub fn merged_schemes(&self) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> = self.schemes.iter().cloned().collect();
        for project_path in &self.project_refs {
            let dir = project_path.join("xcshareddata/xcschemes");
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension() == Some(OsStr::new("xcscheme"))
                    && let Some(name) = p.file_stem().and_then(OsStr::to_str)
                {
                    set.insert(name.to_string());
                }
            }
        }
        set.into_iter().collect()
    }

    /// Locate the `.xcodeproj` member that owns a scheme by name. Returns
    /// the project path whose `xcshareddata/xcschemes/<name>.xcscheme`
    /// exists. Used by callers (the CLI) that need to dispatch a
    /// scheme-driven build to the right project.
    #[must_use]
    pub fn project_for_scheme(&self, scheme_name: &str) -> Option<&Path> {
        for project_path in &self.project_refs {
            let candidate =
                project_path.join(format!("xcshareddata/xcschemes/{scheme_name}.xcscheme"));
            if candidate.exists() {
                return Some(project_path);
            }
        }
        None
    }
}

fn collect_project_refs(element: &Element, base: &Path, out: &mut Vec<PathBuf>) {
    for child in &element.children {
        match child.name.as_str() {
            "FileRef" => {
                let Some(location) = child.attr("location") else {
                    continue;
                };
                let Some(path) = resolve_location(location, base) else {
                    continue;
                };
                if path.extension().and_then(OsStr::to_str) == Some("xcodeproj") {
                    out.push(path);
                }
            }
            "Group" => {
                // A Group's `location` (when present) re-anchors its
                // children. Without one, children resolve against the same
                // base as their parent.
                let group_base = child
                    .attr("location")
                    .and_then(|loc| resolve_location(loc, base))
                    .unwrap_or_else(|| base.to_path_buf());
                collect_project_refs(child, &group_base, out);
            }
            _ => {}
        }
    }
}

/// Resolve a `location="<prefix>:<rest>"` to an absolute path, given the
/// base directory (workspace dir or parent group's resolved location).
fn resolve_location(location: &str, base: &Path) -> Option<PathBuf> {
    let (prefix, rest) = location.split_once(':')?;
    match prefix {
        "container" | "group" | "self" => Some(base.join(rest)),
        "absolute" => Some(PathBuf::from(rest)),
        "developer" => Some(xcode::detect_developer_dir().join(rest)),
        _ => None,
    }
}

fn scan_shared_schemes(workspace_path: &Path) -> Vec<String> {
    let dir = workspace_path.join("xcshareddata/xcschemes");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension() == Some(OsStr::new("xcscheme")) {
                p.file_stem().and_then(OsStr::to_str).map(String::from)
            } else {
                None
            }
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    #[test]
    fn opens_kingfisher_workspace() {
        let ws_path = fixtures_root().join("kingfisher/xcode-26.5.0/raw/Kingfisher.xcworkspace");
        let ws = open(&ws_path).unwrap();
        assert_eq!(ws.name, "Kingfisher");
        let names: Vec<&str> = ws
            .project_refs
            .iter()
            .map(|p| p.file_name().and_then(OsStr::to_str).unwrap_or(""))
            .collect();
        assert_eq!(
            names,
            vec!["Kingfisher.xcodeproj", "Kingfisher-Demo.xcodeproj"]
        );
        // Both referenced projects should resolve to existing directories
        // on disk.
        for p in &ws.project_refs {
            assert!(p.exists(), "expected {} to exist", p.display());
        }
    }

    #[test]
    fn opens_alamofire_workspace_with_three_projects() {
        let ws_path = fixtures_root().join("alamofire/xcode-26.5.0/raw/Alamofire.xcworkspace");
        let ws = open(&ws_path).unwrap();
        let names: Vec<&str> = ws
            .project_refs
            .iter()
            .map(|p| p.file_name().and_then(OsStr::to_str).unwrap_or(""))
            .collect();
        assert_eq!(
            names,
            vec![
                "Alamofire.xcodeproj",
                "iOS Example.xcodeproj",
                "watchOS Example.xcodeproj",
            ]
        );
    }

    #[test]
    fn resolves_location_prefixes() {
        // The base is the directory containing the .xcworkspace bundle.
        let base = PathBuf::from("/tmp/parent");
        assert_eq!(
            resolve_location("container:Foo.xcodeproj", &base),
            Some(PathBuf::from("/tmp/parent/Foo.xcodeproj")),
        );
        assert_eq!(
            resolve_location("group:Sub/Bar.xcodeproj", &base),
            Some(PathBuf::from("/tmp/parent/Sub/Bar.xcodeproj")),
        );
        assert_eq!(
            resolve_location("absolute:/abs/path/Baz.xcodeproj", &base),
            Some(PathBuf::from("/abs/path/Baz.xcodeproj")),
        );
        assert_eq!(
            resolve_location("self:nested", &base),
            Some(PathBuf::from("/tmp/parent/nested")),
        );
        assert!(resolve_location("bogus:nope", &base).is_none());
    }

    #[test]
    fn rejects_non_workspace_root() {
        let scheme_path = fixtures_root().join(
            "kingfisher/xcode-26.5.0/raw/Kingfisher.xcodeproj/xcshareddata/xcschemes/Kingfisher.xcscheme",
        );
        let err = open(scheme_path.parent().unwrap()).unwrap_err();
        // The path doesn't have contents.xcworkspacedata — either I/O error
        // or BadWorkspace, but not Ok.
        assert!(matches!(
            err,
            Error::Io(_) | Error::Parse(_) | Error::BadWorkspace(_)
        ));
    }
}
