use clap::{Args, Subcommand};

use loom_core::error::LoomError;
use loom_core::resolve::registry::{RegistryConfig, RegistryDependencySource};

use crate::ui::{self, Icon};
use crate::GlobalContext;

#[derive(Subcommand)]
pub enum RegistryCommands {
    /// Search the package registry
    Search(SearchArgs),
    /// Publish a component to the registry
    Publish(PublishArgs),
    /// Install a component from the registry
    Install(InstallArgs),
}

#[derive(Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
}

#[derive(Args)]
pub struct PublishArgs {
    /// Path to component root (defaults to current directory)
    #[arg(short, long)]
    pub path: Option<String>,

    /// Registry token (overrides LOOM_REGISTRY_TOKEN)
    #[arg(long)]
    pub token: Option<String>,

    /// Dry run — validate but do not publish
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct InstallArgs {
    /// Package name (e.g., "acmecorp/axi_fifo")
    pub package: String,

    /// Version constraint (e.g., ">=1.0.0")
    #[arg(short, long, default_value = "*")]
    pub version: String,
}

pub fn run(cmd: RegistryCommands, ctx: &GlobalContext) -> Result<(), LoomError> {
    match cmd {
        RegistryCommands::Search(args) => run_search(args, ctx),
        RegistryCommands::Publish(args) => run_publish(args, ctx),
        RegistryCommands::Install(args) => run_install(args, ctx),
    }
}

fn run_search(args: SearchArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let config = load_registry_config(None)?;
    let source = RegistryDependencySource::new(config, get_cache_dir()?);

    if !ctx.quiet {
        ui::status(Icon::Dot, "Search", &format!("'{}'", args.query));
    }

    let results = source.search(&args.query)?;

    if ctx.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_default()
        );
    } else if results.is_empty() {
        ui::status(
            Icon::Dot,
            "Search",
            &format!("no packages found matching '{}'", args.query),
        );
    } else {
        for pkg in &results {
            let desc = pkg.description.as_deref().unwrap_or("(no description)");
            println!("  {} v{} — {}", pkg.name, pkg.version, desc);
        }
    }

    Ok(())
}

fn run_publish(args: PublishArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let component_root = match &args.path {
        Some(p) => std::path::PathBuf::from(p),
        None => std::env::current_dir().map_err(|e| LoomError::Io {
            path: ".".into(),
            source: e,
        })?,
    };

    if !ctx.quiet {
        ui::status(
            Icon::Dot,
            "Publish",
            &format!("preparing {}", component_root.display()),
        );
    }

    // Validate the component manifest first
    let manifest_path = component_root.join("component.toml");
    let manifest = loom_core::manifest::load_component_manifest(&manifest_path)?;
    let errors = manifest.validate();
    if !errors.is_empty() {
        for err in &errors {
            eprintln!("  validation error: {}", err);
        }
        return Err(LoomError::Internal(
            "Component manifest has validation errors".to_string(),
        ));
    }

    if args.dry_run {
        ui::status(
            Icon::Check,
            "Publish",
            &format!(
                "dry run: {} v{} is valid",
                manifest.component.name, manifest.component.version
            ),
        );
        return Ok(());
    }

    let token = args
        .token
        .or_else(|| std::env::var("LOOM_REGISTRY_TOKEN").ok());

    let config = RegistryConfig {
        token,
        ..RegistryConfig::default()
    };

    // Create tarball
    let tmp_dir = std::env::temp_dir().join("loom_publish");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| LoomError::Io {
        path: tmp_dir.clone(),
        source: e,
    })?;

    let tarball = loom_core::resolve::registry::create_package_tarball(&component_root, &tmp_dir)?;

    // Publish
    let source = RegistryDependencySource::new(config, get_cache_dir()?);
    let url = source.publish(&tarball)?;

    if !ctx.quiet {
        ui::status(
            Icon::Check,
            "Published",
            &format!(
                "{} v{}: {}",
                manifest.component.name, manifest.component.version, url
            ),
        );
    }

    Ok(())
}

fn run_install(args: InstallArgs, ctx: &GlobalContext) -> Result<(), LoomError> {
    let config = load_registry_config(None)?;
    let source = RegistryDependencySource::new(config, get_cache_dir()?);

    if !ctx.quiet {
        ui::status(
            Icon::Dot,
            "Install",
            &format!("{} ({})", args.package, args.version),
        );
    }

    // List available versions
    let versions = source.list_versions(&args.package)?;

    if !ctx.quiet && ctx.verbose > 0 {
        eprintln!("    Available versions: {:?}", versions);
    }

    // Download the latest matching version
    let version = versions.first().ok_or_else(|| {
        LoomError::Internal(format!("No versions found for package '{}'", args.package))
    })?;

    let path = source.download(&args.package, version)?;

    if !ctx.quiet {
        ui::status(
            Icon::Check,
            "Installed",
            &format!("{} v{} to {}", args.package, version, path.display()),
        );
    }

    Ok(())
}

fn load_registry_config(token: Option<String>) -> Result<RegistryConfig, LoomError> {
    let token = token.or_else(|| std::env::var("LOOM_REGISTRY_TOKEN").ok());
    Ok(RegistryConfig {
        token,
        ..RegistryConfig::default()
    })
}

fn get_cache_dir() -> Result<std::path::PathBuf, LoomError> {
    let dir = dirs_or_default().join("loom").join("cache");
    Ok(dir)
}

fn dirs_or_default() -> std::path::PathBuf {
    std::env::var("LOOM_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("loom_cache"))
}
