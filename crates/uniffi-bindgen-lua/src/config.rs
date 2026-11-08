//! The `[bindings.lua]` section of `uniffi.toml`.

use serde::Deserialize;

/// Options that customize the generated Lua modules.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Name of the shared library that holds the scaffolding. Informational:
    /// the generated code links against it, it is not loaded at run time.
    pub cdylib_name: Option<String>,
    /// Skip the API checksum comparison at start-up.
    pub omit_checksums: bool,
    /// Renames items for Lua only, in UniFFI's dot notation: `Record = "NewName"`,
    /// `"Record.field" = "new_field"`, `"Object.method.arg" = "new_arg"`.
    pub rename: toml::Table,
}

impl Config {
    /// Reads `[bindings.lua]` from the root of a `uniffi.toml`.
    pub fn from_root_toml(root: &toml::Value) -> anyhow::Result<Self> {
        match root.get("bindings").and_then(|b| b.get("lua")) {
            Some(section) => Ok(section.clone().try_into()?),
            None => Ok(Self::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_root_toml_without_section_is_default() {
        let root: toml::Value =
            toml::from_str("[bindings.kotlin]\npackage_name='x'").expect("toml");
        assert!(
            !Config::from_root_toml(&root)
                .expect("config")
                .omit_checksums
        );
    }

    #[test]
    fn from_root_toml_reads_the_lua_section() {
        let root: toml::Value =
            toml::from_str("[bindings.lua]\nomit_checksums = true").expect("toml");
        assert!(
            Config::from_root_toml(&root)
                .expect("config")
                .omit_checksums
        );
    }

    #[test]
    fn from_root_toml_with_wrong_type_is_an_error() {
        let root: toml::Value =
            toml::from_str("[bindings.lua]\nomit_checksums = 'yes'").expect("toml");
        assert!(Config::from_root_toml(&root).is_err());
    }
}
