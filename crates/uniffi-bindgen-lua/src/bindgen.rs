//! The driver: loads a library or UDL file through UniFFI's loader, parses each
//! component's configuration and hands the components to a [`BindingGenerator`].
//!
//! UniFFI 0.33 dropped its own `BindingGenerator` trait for a loader that every
//! generator drives itself; this module keeps that small contract in one place.

use anyhow::{Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use uniffi_bindgen::{BindgenLoader, BindgenPaths, CargoMetadataOptions, Component, GlobalConfig};

/// What a generator is told about the run.
#[derive(Debug, Clone)]
pub struct GenerationSettings {
    /// Directory to write the generated files to.
    pub out_dir: Utf8PathBuf,
    /// Run a formatter over the output, when one is installed.
    pub try_format_code: bool,
    /// Name of the library the definitions came from, when known.
    pub cdylib: Option<String>,
}

/// A language backend.
pub trait BindingGenerator {
    /// The language's configuration, from `[bindings.<language>]` of `uniffi.toml`.
    type Config: Default;

    /// Parses the whole `uniffi.toml` of one crate.
    fn new_config(&self, root_toml: &toml::Value) -> Result<Self::Config>;

    /// Adjusts the configurations once every component is known.
    fn update_component_configs(
        &self,
        settings: &GenerationSettings,
        components: &mut Vec<Component<Self::Config>>,
    ) -> Result<()>;

    /// Writes the bindings of every component.
    fn write_bindings(
        &self,
        settings: &GenerationSettings,
        components: &[Component<Self::Config>],
    ) -> Result<()>;
}

/// Where the definitions come from and where the bindings go.
#[derive(Debug, Clone)]
pub struct GenerateRequest<'a> {
    /// A library built with UniFFI, or a `.udl` file.
    pub source: &'a Utf8Path,
    /// Directory to write to.
    pub out_dir: &'a Utf8Path,
    /// A `uniffi.toml` that overrides the one of the crate.
    pub config: Option<&'a Utf8Path>,
    /// Only write the bindings of this crate.
    pub crate_filter: Option<&'a str>,
    /// Run a formatter over the output.
    pub format: bool,
    /// Do not include dependencies when running `cargo metadata`.
    pub metadata_no_deps: bool,
}

/// Loads `request.source` and writes its bindings with `generator`.
pub fn generate<G: BindingGenerator>(generator: &G, request: &GenerateRequest<'_>) -> Result<()> {
    let mut paths = BindgenPaths::default();
    let global_config = match request.config {
        Some(path) => {
            let (config, roots) = GlobalConfig::from_file(path)?;
            if let Some(layer) = roots {
                paths.add_layer(layer);
            }
            config
        }
        None => GlobalConfig::default(),
    };
    paths.add_cargo_metadata_layer(CargoMetadataOptions {
        no_deps: request.metadata_no_deps,
        ..CargoMetadataOptions::default()
    })?;
    let loader = BindgenLoader::new(paths, global_config);

    let metadata = loader.load_metadata(request.source)?;
    if let Some(filter) = request.crate_filter
        && !metadata.contains_key(filter)
    {
        bail!("no UniFFI metadata found for crate {filter}");
    }
    let cis = loader.load_cis(request.source, metadata)?;
    let mut components = loader.load_components(cis, |_, toml| generator.new_config(&toml))?;
    let settings = GenerationSettings {
        out_dir: request.out_dir.to_owned(),
        try_format_code: request.format,
        cdylib: loader.library_name(request.source),
    };
    generator.update_component_configs(&settings, &mut components)?;
    for component in &mut components {
        component.ci.derive_ffi_funcs()?;
    }
    if let Some(filter) = request.crate_filter {
        components.retain(|c| c.ci.crate_name() == filter);
    }
    std::fs::create_dir_all(&settings.out_dir)?;
    generator.write_bindings(&settings, &components)
}
