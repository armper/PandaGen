//! PandaGen package format and component loader.
//!
//! Packages describe components declaratively in a JSON manifest. The loader
//! validates the manifest and produces a launch plan for the workspace.

use resources::ResourceBudget;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const PACKAGE_MANIFEST_NAME: &str = "pandagend.json";

/// Package manifest format version.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageFormatVersion {
    pub major: u32,
    pub minor: u32,
}

impl PackageFormatVersion {
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

/// Top-level package manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub format_version: PackageFormatVersion,
    pub name: String,
    pub version: String,
    pub components: Vec<ComponentSpec>,
}

impl PackageManifest {
    pub fn validate(&self) -> Result<(), PackageError> {
        if self.name.trim().is_empty() {
            return Err(PackageError::InvalidManifest(
                "Package name cannot be empty".to_string(),
            ));
        }

        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for component in &self.components {
            if !ids.insert(component.id.clone()) {
                return Err(PackageError::DuplicateComponentId(component.id.clone()));
            }
            if !names.insert(component.name.clone()) {
                return Err(PackageError::DuplicateComponentName(component.name.clone()));
            }
            if component.entry.trim().is_empty() {
                return Err(PackageError::InvalidManifest(format!(
                    "Component {} has empty entry",
                    component.id
                )));
            }
        }

        Ok(())
    }
}

/// Component type declared in the package.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PackageComponentType {
    Editor,
    Cli,
    PipelineExecutor,
    Custom,
}

/// Component specification within a package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentSpec {
    pub id: String,
    pub name: String,
    pub component_type: PackageComponentType,
    pub entry: String,
    #[serde(default = "default_focusable")]
    pub focusable: bool,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    #[serde(default)]
    pub budget: Option<ResourceBudget>,
}

fn default_focusable() -> bool {
    true
}

/// Launch specification derived from a package component.
#[derive(Debug, Clone)]
pub struct ComponentLaunchSpec {
    pub id: String,
    pub name: String,
    pub component_type: PackageComponentType,
    pub entry: String,
    pub focusable: bool,
    pub metadata: HashMap<String, String>,
    pub budget: Option<ResourceBudget>,
}

/// Errors related to loading or validating packages.
#[derive(Debug, Error)]
pub enum PackageError {
    #[error("Package manifest not found: {0}")]
    ManifestNotFound(String),

    #[error("Failed to read manifest: {0}")]
    Io(String),

    #[error("Failed to parse manifest: {0}")]
    Parse(String),

    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("Duplicate component id: {0}")]
    DuplicateComponentId(String),

    #[error("Duplicate component name: {0}")]
    DuplicateComponentName(String),
}

/// Loads package manifests from disk.
pub struct PackageLoader;

impl PackageLoader {
    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<PackageManifest, PackageError> {
        let manifest_path = PathBuf::from(dir.as_ref()).join(PACKAGE_MANIFEST_NAME);
        if !manifest_path.exists() {
            return Err(PackageError::ManifestNotFound(
                manifest_path.display().to_string(),
            ));
        }
        Self::load_from_path(manifest_path)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<PackageManifest, PackageError> {
        let data = fs::read_to_string(path.as_ref())
            .map_err(|err| PackageError::Io(err.to_string()))?;
        let manifest: PackageManifest =
            serde_json::from_str(&data).map_err(|err| PackageError::Parse(err.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }
}

/// Builds launch plans from a manifest.
pub struct ComponentLoader;

impl ComponentLoader {
    pub fn build_launch_plan(
        manifest: &PackageManifest,
    ) -> Result<Vec<ComponentLaunchSpec>, PackageError> {
        manifest.validate()?;
        Ok(manifest
            .components
            .iter()
            .map(|component| ComponentLaunchSpec {
                id: component.id.clone(),
                name: component.name.clone(),
                component_type: component.component_type,
                entry: component.entry.clone(),
                focusable: component.focusable,
                metadata: component.metadata.clone(),
                budget: component.budget,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_load_manifest() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join(PACKAGE_MANIFEST_NAME);

        let manifest = r#"
        {
          "format_version": { "major": 1, "minor": 0 },
          "name": "demo",
          "version": "0.1.0",
          "components": [
            {
              "id": "editor",
              "name": "Demo Editor",
              "component_type": "editor",
              "entry": "services_editor_vi",
              "focusable": true,
              "metadata": { "role": "primary" }
            }
          ]
        }
        "#;

        fs::write(&manifest_path, manifest).unwrap();
        let loaded = PackageLoader::load_from_dir(dir.path()).unwrap();

        assert_eq!(loaded.name, "demo");
        assert_eq!(loaded.components.len(), 1);
        assert_eq!(loaded.components[0].id, "editor");
    }

    #[test]
    fn test_duplicate_component_ids_fail() {
        let manifest = PackageManifest {
            format_version: PackageFormatVersion::new(1, 0),
            name: "dup".to_string(),
            version: "0.1.0".to_string(),
            components: vec![
                ComponentSpec {
                    id: "comp".to_string(),
                    name: "One".to_string(),
                    component_type: PackageComponentType::Editor,
                    entry: "services_editor_vi".to_string(),
                    focusable: true,
                    metadata: HashMap::new(),
                    budget: None,
                },
                ComponentSpec {
                    id: "comp".to_string(),
                    name: "Two".to_string(),
                    component_type: PackageComponentType::Cli,
                    entry: "cli_console".to_string(),
                    focusable: true,
                    metadata: HashMap::new(),
                    budget: None,
                },
            ],
        };

        let result = manifest.validate();
        assert!(matches!(result, Err(PackageError::DuplicateComponentId(_))));
    }

    #[test]
    fn test_component_loader_builds_plan() {
        let manifest = PackageManifest {
            format_version: PackageFormatVersion::new(1, 0),
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            components: vec![ComponentSpec {
                id: "editor".to_string(),
                name: "Demo".to_string(),
                component_type: PackageComponentType::Editor,
                entry: "services_editor_vi".to_string(),
                focusable: true,
                metadata: HashMap::new(),
                budget: None,
            }],
        };

        let plan = ComponentLoader::build_launch_plan(&manifest).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].name, "Demo");
        assert_eq!(plan[0].entry, "services_editor_vi");
    }
}
