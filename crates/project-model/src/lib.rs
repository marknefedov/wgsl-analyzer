//! In `wgsl-analyzer`, we maintain a strict separation between pure abstract
//! semantic project model and a concrete model of a particular build system.
//!
//! Pure model is represented by the [`PackageGraph`] from another package.
//!
//! In this crate, we are concerned with "real world" project models.
//!
//! Specifically, here we have a representation for a `wesl-rs` project
//! ([`WeslManifest`]) and for manually specified layout ([`ProjectManifest::ProjectJson`]).
//!
//! Roughly, the things we do here are:
//!
//! * Project discovery (where is the relevant `wesl.toml` for the current directory?)
//! * Lowering of concrete model to a [`PackageGraph`]

mod manifest_path;
mod package_graph;
mod package_interner;
mod wesl_package;
mod wesl_toml;
use std::{fmt, fs};

use anyhow::{Context as _, bail, format_err};
use base_db::input::PackageOrigin;
use cargo_metadata::MetadataCommand;
pub use manifest_path::ManifestPath;
pub use package_graph::{PackageChange, PackageGraph, PackageKey};
use paths::{AbsPath, AbsPathBuf};
pub use wesl_package::{PackageDependency, WeslPackage, WeslPackageRoot};
pub use wesl_toml::{WeslDependency, WeslManifest};

/// Points at a relevant manifest file on disk.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ProjectManifest {
    /// Uses `wesl-project.json`.
    ProjectJson(ManifestPath),
    /// Uses the `package.metadata.wgsl-analyzer` table as a stand-in for `wesl.toml`.
    CargoToml(ManifestPath),
    /// A `wesl.toml` file.
    WeslToml(ManifestPath),
}

impl ProjectManifest {
    pub fn from_manifest_file(path: AbsPathBuf) -> anyhow::Result<Self> {
        let path = ManifestPath::try_from(path)
            .map_err(|path| format_err!("bad manifest path: {path}"))?;
        Self::from_manifest_path(path)
    }

    #[must_use]
    pub fn discover<CargoTomlFilter>(
        path: &AbsPath,
        search_parents: bool,
        mut cargo_filter: CargoTomlFilter,
    ) -> Option<Self>
    where
        CargoTomlFilter: FnMut(&ManifestPath) -> bool,
    {
        let target_file_names = [
            "wesl-project.json",
            ".wesl-project.json",
            "wesl.toml",
            "Cargo.toml",
        ];
        let mut eligible = |manifest: &Self| match manifest {
            Self::CargoToml(path) => cargo_filter(path),
            Self::WeslToml(_) | Self::ProjectJson(_) => true,
        };
        for target_file_name in target_file_names {
            if path.file_name().unwrap_or_default() == target_file_name
                && let Ok(Ok(manifest)) =
                    ManifestPath::try_from(path.to_path_buf()).map(Self::from_manifest_path)
                && eligible(&manifest)
            {
                return Some(manifest);
            }
        }
        let mut curr = Some(path);
        while let Some(path) = curr {
            let candidate = target_file_names
                .iter()
                .map(|target_file_name| path.join(target_file_name))
                .filter(|candidate| fs::metadata(candidate).is_ok())
                .filter_map(|candidate| ManifestPath::try_from(candidate).ok())
                .filter_map(|candidate| Self::from_manifest_path(candidate).ok())
                .find(&mut eligible);
            if let Some(manifest) = candidate {
                return Some(manifest);
            }
            if search_parents {
                curr = path.parent();
            } else {
                return None;
            }
        }
        None
    }

    #[must_use]
    pub const fn manifest_path(&self) -> &ManifestPath {
        match self {
            Self::ProjectJson(manifest) | Self::WeslToml(manifest) | Self::CargoToml(manifest) => {
                manifest
            },
        }
    }

    pub fn from_manifest_path(path: ManifestPath) -> anyhow::Result<Self> {
        let file_name = path.file_name().context("path must have a file name")?;
        Ok(match file_name {
            "wesl-project.json" | ".wesl-project.json" => Self::ProjectJson(path),
            "Cargo.toml" => Self::CargoToml(path),
            "wesl.toml" => Self::WeslToml(path),
            _ => bail!("project root must point to a wesl.toml or wesl-project.json file: {path}"),
        })
    }
}

#[cfg(test)]
mod discovery_tests {
    use super::*;

    fn fixture_root() -> AbsPathBuf {
        AbsPathBuf::assert_utf8(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/discovery"),
        )
    }

    #[test]
    fn skips_ineligible_cargo_manifests_and_continues_to_parent() {
        let root = fixture_root();
        let mut visited = Vec::new();
        let result = ProjectManifest::discover(&root.join("outer/inner"), true, |path| {
            visited.push(path.clone());
            path.as_str() == root.join("outer/Cargo.toml").normalize().as_str()
        })
        .unwrap();
        assert_eq!(
            result.manifest_path().as_str(),
            root.join("outer/Cargo.toml").normalize().as_str()
        );
        assert_eq!(visited.len(), 2);
    }

    #[test]
    fn rejected_cargo_does_not_hide_parent_wesl() {
        let root = fixture_root();
        let result = ProjectManifest::discover(&root.join("outer/inner"), true, |_| false).unwrap();
        assert_eq!(
            result.manifest_path().as_str(),
            root.join("wesl.toml").normalize().as_str()
        );
    }

    #[test]
    fn explicit_cargo_manifest_is_also_filtered() {
        let root = fixture_root();
        let result =
            ProjectManifest::discover(&root.join("outer/inner/Cargo.toml"), true, |_| false)
                .unwrap();
        assert_eq!(
            result.manifest_path().as_str(),
            root.join("wesl.toml").normalize().as_str()
        );
    }

    #[test]
    fn does_not_search_parents_when_disabled() {
        let root = fixture_root();
        assert!(ProjectManifest::discover(&root.join("outer/inner"), false, |_| false).is_none());
        assert!(
            ProjectManifest::discover(&root.join("outer/inner/Cargo.toml"), false, |_| false)
                .is_none()
        );
    }

    #[test]
    fn accepts_eligible_cargo_and_wesl_manifests() {
        let root = fixture_root();
        let cargo = root.join("outer/inner/Cargo.toml").normalize();
        let result = ProjectManifest::discover(&cargo, false, |_| true).unwrap();
        assert_eq!(result.manifest_path().as_str(), cargo.as_str());
        let result = ProjectManifest::discover(&root, false, |_| {
            panic!("WESL must not call the Cargo filter")
        })
        .unwrap();
        assert_eq!(
            result.manifest_path().as_str(),
            root.join("wesl.toml").normalize().as_str()
        );
    }
}

impl fmt::Display for ProjectManifest {
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        fmt::Display::fmt(self.manifest_path(), formatter)
    }
}

/// `PackageRoot` describes a package root folder.
/// Which may be an external dependency, or a member of
/// the current workspace.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct PackageRoot {
    pub origin: PackageOrigin,
    pub manifest: ManifestPath,
    /// Files to include.
    pub include_files: Vec<AbsPathBuf>,
    /// Directories to include.
    pub include: Vec<AbsPathBuf>,
    /// Directories to exclude.
    pub exclude: Vec<AbsPathBuf>,
}
