use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use sweetpad::build_context::{BuildContext, ResolveQuery, Resolved};
use sweetpad::destination::RunDestination;
use sweetpad::xcspec::Catalog;
use sweetpad::{
    catalog_cache, pbxproj, project, resolver, scheme, workspace, xcconfig, xcode, xcscheme,
};

#[derive(Parser)]
#[command(name = "sweetpad", version, about = "Xcode project tooling", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a project.pbxproj file and print its AST.
    ParsePbxproj {
        /// Path to the project.pbxproj file.
        path: PathBuf,
    },
    /// Parse an .xcconfig file and print its entries.
    ParseXcconfig {
        /// Path to the .xcconfig file.
        path: PathBuf,
    },
    /// Parse an .xcscheme file and print its element tree.
    ParseXcscheme {
        /// Path to the .xcscheme file.
        path: PathBuf,
    },
    /// List a project's or workspace's targets, configurations, and shared
    /// schemes. Mirrors `xcodebuild -list -project|-workspace <path>`.
    List(ListArgs),
    /// Resolved build settings for a target or scheme. Mirrors
    /// `xcodebuild -showBuildSettings -project|-workspace <path>
    /// [-target T | -scheme S] -configuration C`.
    BuildSettings(BuildSettingsArgs),
    /// Active Xcode information: developer dir, short version, build
    /// version. Mirrors `xcrun xcodebuild -version` in text form, with a
    /// `--json` option for machine consumption.
    XcodeVersion {
        /// Emit JSON `{developerDir, shortVersion, buildVersion, majorVersion}`.
        #[arg(long)]
        json: bool,
    },
    /// Resolve an .xcconfig (following #includes) under a build context.
    /// Prints one `KEY=VALUE` pair per line, sorted.
    Resolve {
        /// Path to the root .xcconfig.
        path: PathBuf,
        /// SDK to match conditional assignments against.
        #[arg(long, default_value = "macosx")]
        sdk: String,
        /// Architecture to match conditional assignments against.
        #[arg(long, default_value = "arm64")]
        arch: String,
        /// Build configuration to match conditional assignments against.
        #[arg(long = "config", default_value = "Debug")]
        configuration: String,
    },
}

#[derive(Args)]
struct ListArgs {
    /// Path to the .xcodeproj. Mutually exclusive with --workspace.
    #[arg(
        long,
        conflicts_with = "workspace",
        required_unless_present = "workspace"
    )]
    project: Option<PathBuf>,
    /// Path to the .xcworkspace. Mutually exclusive with --project.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Emit JSON matching `xcodebuild -list -json` output shape.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct BuildSettingsArgs {
    /// Path to the .xcodeproj. Mutually exclusive with --workspace.
    #[arg(
        long,
        conflicts_with = "workspace",
        required_unless_present = "workspace"
    )]
    project: Option<PathBuf>,
    /// Path to the .xcworkspace. Mutually exclusive with --project.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Target name. Mutually exclusive with --scheme.
    #[arg(long, conflicts_with = "scheme", required_unless_present = "scheme")]
    target: Option<String>,
    /// Scheme name — resolves every buildable in the scheme's BuildAction.
    /// Mutually exclusive with --target.
    #[arg(long)]
    scheme: Option<String>,
    /// Configuration (e.g. `Debug`, `Release`).
    #[arg(long = "config")]
    configuration: String,
    /// SDK to bind conditional assignments to.
    #[arg(long, default_value = "macosx")]
    sdk: String,
    /// Architecture to bind conditional assignments to.
    #[arg(long, default_value = "arm64")]
    arch: String,
    /// Additional .xcconfig overlay (`xcodebuild -xcconfig FILE`).
    #[arg(long)]
    xcconfig: Option<PathBuf>,
    /// Directory containing Apple's `*.xcspec` files. When omitted, the
    /// defaults catalog baked into the binary (latest captured Xcode) is used.
    #[arg(long)]
    xcspec_root: Option<PathBuf>,
    /// Directory containing per-SDK `SDKSettings.plist`.
    #[arg(long)]
    sdksettings_root: Option<PathBuf>,
    /// Cache file for the parsed `--xcspec-root` catalog. Defaults to a path in
    /// the OS cache dir keyed by the xcspec root; ignored without `--xcspec-root`.
    #[arg(long)]
    catalog_cache: Option<PathBuf>,
    /// `xcodebuild -derivedDataPath PATH` — overrides the computed
    /// DerivedData root for `BUILD_DIR`, `OBJROOT`, and friends.
    #[arg(long)]
    derived_data_path: Option<PathBuf>,
    /// Emit JSON matching `xcodebuild -showBuildSettings -json` output.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::ParsePbxproj { path } => {
            let value = pbxproj::parse_file(&path).map_err(|e| e.to_string())?;
            println!("{value:#?}");
        }
        Command::ParseXcconfig { path } => {
            let xcc = xcconfig::parse_file(&path).map_err(|e| e.to_string())?;
            println!("{xcc:#?}");
        }
        Command::ParseXcscheme { path } => {
            let element = xcscheme::parse_file(&path).map_err(|e| e.to_string())?;
            println!("{element:#?}");
        }
        Command::List(args) => run_list(&args)?,
        Command::BuildSettings(args) => run_build_settings(&args)?,
        Command::XcodeVersion { json } => run_xcode_version(json),
        Command::Resolve {
            path,
            sdk,
            arch,
            configuration,
        } => {
            let assignments = resolver::flatten_xcconfig(&path).map_err(|e| e.to_string())?;
            let ctx = resolver::ResolveContext {
                sdk,
                arch,
                configuration,
                variant: String::new(),
            };
            let resolved = resolver::resolve(&[&assignments], &ctx);
            for (key, value) in &resolved {
                println!("{key}={value}");
            }
        }
    }
    Ok(())
}

// -- list --------------------------------------------------------------------

fn run_list(args: &ListArgs) -> Result<(), String> {
    if let Some(workspace_path) = &args.workspace {
        let ws = workspace::open(workspace_path).map_err(|e| e.to_string())?;
        if args.json {
            print_workspace_json(&ws);
        } else {
            print_workspace_text(&ws);
        }
    } else if let Some(project_path) = &args.project {
        let project = project::open(project_path).map_err(|e| e.to_string())?;
        if args.json {
            print_project_json(&project);
        } else {
            print_project_text(&project);
        }
    }
    Ok(())
}

fn print_project_text(project: &project::Project) {
    println!("Information about project \"{}\":", project.name);
    println!("    Targets:");
    if project.targets.is_empty() {
        println!("        (none)");
    } else {
        for t in &project.targets {
            println!("        {}", t.name);
        }
    }
    println!();
    println!("    Build Configurations:");
    if project.configurations.is_empty() {
        println!("        (none)");
    } else {
        for c in &project.configurations {
            println!("        {c}");
        }
    }
    println!();
    println!("    Schemes:");
    if project.schemes.is_empty() {
        println!("        (none)");
    } else {
        for s in &project.schemes {
            println!("        {s}");
        }
    }
}

fn print_workspace_text(ws: &workspace::Workspace) {
    println!("Information about workspace \"{}\":", ws.name);
    println!("    Schemes:");
    let schemes = ws.merged_schemes();
    if schemes.is_empty() {
        println!("        (none)");
    } else {
        for s in &schemes {
            println!("        {s}");
        }
    }
}

fn print_project_json(project: &project::Project) {
    let mut buf = String::new();
    buf.push_str("{\n  \"project\": {\n");
    let target_names: Vec<String> = project.targets.iter().map(|t| t.name.clone()).collect();
    let _ = writeln!(
        buf,
        "    \"configurations\": {},",
        json_string_array(&project.configurations)
    );
    let _ = writeln!(buf, "    \"name\": {},", json_string(&project.name));
    let _ = writeln!(
        buf,
        "    \"schemes\": {},",
        json_string_array(&project.schemes)
    );
    let _ = writeln!(buf, "    \"targets\": {}", json_string_array(&target_names));
    buf.push_str("  }\n}");
    println!("{buf}");
}

fn print_workspace_json(ws: &workspace::Workspace) {
    let mut buf = String::new();
    buf.push_str("{\n  \"workspace\": {\n");
    let _ = writeln!(buf, "    \"name\": {},", json_string(&ws.name));
    let _ = writeln!(
        buf,
        "    \"schemes\": {}",
        json_string_array(&ws.merged_schemes())
    );
    buf.push_str("  }\n}");
    println!("{buf}");
}

// -- build-settings ----------------------------------------------------------

fn run_build_settings(args: &BuildSettingsArgs) -> Result<(), String> {
    let catalog = load_catalog(
        args.xcspec_root.as_deref(),
        args.sdksettings_root.as_deref(),
        args.catalog_cache.as_deref(),
    )?;
    let projects = resolve_project_paths(args.project.as_deref(), args.workspace.as_deref())?;

    let want_scheme = args.scheme.as_deref();
    let want_target = args.target.as_deref();
    let output = if projects.len() == 1 {
        // Single project: bubble up the underlying error directly so
        // callers get xcodebuild-equivalent messages ("no target named …").
        let ctx = build_one_context(&projects[0], catalog.as_ref(), args.xcconfig.as_deref())?;
        let queries = build_queries(&ctx, args, want_scheme, want_target);
        let mut out = Vec::new();
        for query in queries {
            let resolved = ctx.resolve(&query).map_err(|e| e.to_string())?;
            out.push(TargetResolved {
                target: query.target.clone(),
                resolved,
            });
        }
        out
    } else {
        // Workspace: try every project until queries resolve. Resolve
        // errors here mean the target isn't in that particular project,
        // so we suppress them; we only fail if no project matches at all.
        let mut out = Vec::new();
        for project_path in &projects {
            let ctx = build_one_context(project_path, catalog.as_ref(), args.xcconfig.as_deref())?;
            let queries = build_queries(&ctx, args, want_scheme, want_target);
            for query in queries {
                if let Ok(r) = ctx.resolve(&query) {
                    out.push(TargetResolved {
                        target: query.target.clone(),
                        resolved: r,
                    });
                }
            }
            if !out.is_empty() && want_scheme.is_none() {
                break;
            }
        }
        if out.is_empty() {
            let needle = want_target
                .map(|t| format!("--target {t}"))
                .or_else(|| want_scheme.map(|s| format!("--scheme {s}")))
                .unwrap_or_default();
            return Err(format!(
                "no target matched {needle} across the supplied workspace"
            ));
        }
        out
    };

    if args.json {
        print_build_settings_json(&output);
    } else {
        print_build_settings_text(&output);
    }
    Ok(())
}

struct TargetResolved {
    target: String,
    resolved: Resolved,
}

/// The defaults catalog for a `build-settings` run.
///
/// With `--xcspec-root`, parse that tree (cached to a serialized file so repeat
/// runs skip the ~32 ms walk). Without it, return the catalog baked into the
/// binary for the latest captured Xcode — so the resolver always has Apple's
/// defaults to layer under the project's settings.
fn load_catalog(
    xcspec_root: Option<&Path>,
    sdksettings_root: Option<&Path>,
    catalog_cache: Option<&Path>,
) -> Result<Option<Catalog>, String> {
    let catalog = if let Some(xcspec_dir) = xcspec_root {
        catalog_cache::load_cached_or_build(xcspec_dir, sdksettings_root, catalog_cache)
            .map_err(|e| e.to_string())?
    } else {
        catalog_cache::embedded().map_err(|e| e.to_string())?
    };
    Ok(Some(catalog))
}

/// Resolve the `--project` / `--workspace` selector to the concrete list
/// of `.xcodeproj` paths to try in order.
fn resolve_project_paths(
    project: Option<&Path>,
    workspace_path: Option<&Path>,
) -> Result<Vec<PathBuf>, String> {
    if let Some(ws_path) = workspace_path {
        let ws = workspace::open(ws_path).map_err(|e| e.to_string())?;
        Ok(ws.project_refs)
    } else if let Some(p) = project {
        Ok(vec![p.to_path_buf()])
    } else {
        Err("either --project or --workspace is required".into())
    }
}

fn build_one_context(
    project_path: &Path,
    catalog: Option<&Catalog>,
    extra_xcconfig: Option<&Path>,
) -> Result<BuildContext, String> {
    let mut ctx = BuildContext::open(project_path).map_err(|e| e.to_string())?;
    if let Some(c) = catalog {
        ctx = ctx.with_xcspec(c.clone());
    }
    if let Some(p) = extra_xcconfig {
        ctx = ctx.with_extra_xcconfig(p).map_err(|e| e.to_string())?;
    }
    Ok(ctx)
}

/// Decide which target(s) to resolve, given the user's `--scheme` or
/// `--target` choice. For `--scheme`, look up the scheme XML on disk under
/// the project's `xcshareddata/xcschemes/`.
fn build_queries(
    ctx: &BuildContext,
    args: &BuildSettingsArgs,
    want_scheme: Option<&str>,
    want_target: Option<&str>,
) -> Vec<ResolveQuery> {
    let mut queries = Vec::new();
    if let Some(scheme_name) = want_scheme {
        let scheme_path = ctx
            .project
            .path
            .join("xcshareddata/xcschemes")
            .join(format!("{scheme_name}.xcscheme"));
        let Ok(parsed) = scheme::parse_file(&scheme_path) else {
            return queries;
        };
        let plan = ctx.plan_build(
            &parsed,
            &args.configuration,
            &args.sdk,
            &args.arch,
            None::<&RunDestination>,
        );
        for mut q in plan.entries {
            if let Some(p) = &args.derived_data_path {
                q = q.with_derived_data_path(p.clone());
            }
            queries.push(q);
        }
    } else if let Some(target_name) = want_target {
        let mut q = ResolveQuery::new(target_name, &args.configuration, &args.sdk, &args.arch);
        if let Some(p) = &args.derived_data_path {
            q = q.with_derived_data_path(p.clone());
        }
        queries.push(q);
    }
    queries
}

fn print_build_settings_text(output: &[TargetResolved]) {
    for entry in output {
        println!(
            "Build settings for action build and target {}:",
            entry.target
        );
        for (key, value) in &entry.resolved.settings {
            println!("    {key} = {value}");
        }
        println!();
    }
}

fn print_build_settings_json(output: &[TargetResolved]) {
    let mut buf = String::from("[\n");
    for (i, entry) in output.iter().enumerate() {
        buf.push_str("  {\n");
        buf.push_str("    \"action\": \"build\",\n");
        buf.push_str("    \"buildSettings\": ");
        buf.push_str(&json_string_map(&entry.resolved.settings, 4));
        buf.push_str(",\n");
        let _ = writeln!(buf, "    \"target\": {}", json_string(&entry.target));
        buf.push_str("  }");
        if i + 1 < output.len() {
            buf.push(',');
        }
        buf.push('\n');
    }
    buf.push(']');
    println!("{buf}");
}

// -- xcode-version -----------------------------------------------------------

fn run_xcode_version(json: bool) {
    let info = xcode::active_install();
    if json {
        let mut buf = String::from("{\n");
        let _ = writeln!(
            buf,
            "  \"developerDir\": {},",
            json_string(&info.developer_dir.display().to_string())
        );
        let _ = writeln!(
            buf,
            "  \"shortVersion\": {},",
            json_string(&info.short_version)
        );
        let _ = writeln!(
            buf,
            "  \"buildVersion\": {},",
            json_string(&info.build_version)
        );
        let _ = writeln!(buf, "  \"majorVersion\": {}", info.major_version());
        buf.push('}');
        println!("{buf}");
    } else {
        let short = if info.short_version.is_empty() {
            "Unknown".to_string()
        } else {
            info.short_version.clone()
        };
        let build = if info.build_version.is_empty() {
            "Unknown".to_string()
        } else {
            info.build_version.clone()
        };
        println!("Xcode {short}");
        println!("Build version {build}");
    }
}

// -- tiny JSON encoder -------------------------------------------------------
//
// xcodebuild's JSON output uses a tight subset (no numbers, booleans, or
// nested objects deeper than two levels). A hand-rolled encoder covering
// strings, string arrays, and string→string maps is enough and avoids a
// `serde_json` dependency for this CLI.

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_string_array(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".into();
    }
    let mut out = String::from("[");
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&json_string(s));
    }
    out.push(']');
    out
}

fn json_string_map(map: &BTreeMap<String, String>, indent: usize) -> String {
    if map.is_empty() {
        return "{}".into();
    }
    let pad = " ".repeat(indent);
    let inner_pad = " ".repeat(indent + 2);
    let mut out = String::from("{\n");
    let mut first = true;
    for (k, v) in map {
        if !first {
            out.push_str(",\n");
        }
        first = false;
        let _ = write!(out, "{inner_pad}{}: {}", json_string(k), json_string(v));
    }
    out.push('\n');
    out.push_str(&pad);
    out.push('}');
    out
}
