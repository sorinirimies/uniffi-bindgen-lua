//! Command line interface.

use anyhow::{Context, Result, bail};
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use uniffi_bindgen::cargo_metadata::CrateConfigSupplier;

use crate::codegen::{LuaBindingGenerator, write_runtime};

/// Lua bindings generator for UniFFI.
#[derive(Debug, Parser)]
#[command(name = "uniffi-bindgen-lua", version)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate Lua bindings (C modules).
    Generate {
        /// A cdylib/staticlib built with UniFFI (library mode), or a UDL file.
        source: Utf8PathBuf,

        /// Directory to write the generated files to.
        #[arg(short, long)]
        out_dir: Option<Utf8PathBuf>,

        /// A `uniffi.toml` that overrides the one in the crate.
        #[arg(short, long)]
        config: Option<Utf8PathBuf>,

        /// Only generate bindings for this crate (library mode).
        #[arg(long = "crate")]
        crate_name: Option<String>,

        /// The library to read definitions from when `source` is a UDL file.
        #[arg(long)]
        library: Option<Utf8PathBuf>,

        /// Do not run `clang-format` on the output.
        #[arg(long)]
        no_format: bool,

        /// Do not include dependencies when running `cargo metadata`.
        #[arg(long)]
        metadata_no_deps: bool,
    },
    /// Write only the runtime header, for projects that generate through another tool.
    Runtime {
        /// Directory to write `uniffi_runtime.h` and `uniffi_lua.h` to.
        out_dir: Utf8PathBuf,
    },
}

/// Parses the process arguments and runs the command.
pub fn run() -> Result<()> {
    execute(Cli::parse())
}

fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Generate {
            source,
            out_dir,
            config,
            crate_name,
            library,
            no_format,
            metadata_no_deps,
        } => generate(
            &source,
            out_dir,
            config.as_deref(),
            crate_name,
            library.as_deref(),
            !no_format,
            metadata_no_deps,
        ),
        Command::Runtime { out_dir } => write_runtime(&out_dir),
    }
}

fn generate(
    source: &camino::Utf8Path,
    out_dir: Option<Utf8PathBuf>,
    config: Option<&camino::Utf8Path>,
    crate_name: Option<String>,
    library: Option<&camino::Utf8Path>,
    format: bool,
    metadata_no_deps: bool,
) -> Result<()> {
    let generator = LuaBindingGenerator;
    if uniffi_bindgen::is_cdylib(source) {
        let out_dir = out_dir.context("--out-dir is required when generating from a library")?;
        let supplier = CrateConfigSupplier::from_cargo_metadata_command(metadata_no_deps)?;
        uniffi_bindgen::library_mode::generate_bindings(
            source, crate_name, &generator, &supplier, config, &out_dir, format,
        )?;
        return Ok(());
    }
    if source.extension() != Some("udl") {
        bail!("`{source}` is neither a library nor a .udl file");
    }
    uniffi_bindgen::generate_external_bindings(
        &generator,
        source,
        config,
        out_dir.as_deref(),
        library,
        crate_name.as_deref(),
        format,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("uniffi-bindgen-lua").chain(args.iter().copied()))
    }

    #[test]
    fn generate_parses_every_option() {
        let cli = parse(&[
            "generate",
            "libx.so",
            "--out-dir",
            "out",
            "--config",
            "u.toml",
            "--crate",
            "x",
            "--no-format",
            "--metadata-no-deps",
        ])
        .unwrap();
        let Command::Generate {
            source,
            out_dir,
            config,
            crate_name,
            library,
            no_format,
            metadata_no_deps,
        } = cli.command
        else {
            panic!("expected generate");
        };
        assert_eq!(source, "libx.so");
        assert_eq!(out_dir.as_deref(), Some("out".into()));
        assert_eq!(config.as_deref(), Some("u.toml".into()));
        assert_eq!(crate_name.as_deref(), Some("x"));
        assert_eq!(library, None);
        assert!(no_format && metadata_no_deps);
    }

    #[test]
    fn generate_without_source_is_a_usage_error() {
        assert!(parse(&["generate"]).is_err());
    }

    #[test]
    fn generate_from_library_without_out_dir_is_an_error() {
        let cli = parse(&["generate", "libmissing.so"]).unwrap();
        let error = execute(cli).unwrap_err();
        assert!(
            error.to_string().contains("--out-dir is required"),
            "{error}"
        );
    }
}
