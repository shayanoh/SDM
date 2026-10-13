//! Joining a filename or package name onto a folder without leaving it.

use std::path::{Path, PathBuf};

/// `join` for a name that came from outside the engine.
///
/// `Path::join` with an absolute name replaces the folder and `..` walks out
/// of it, so every filename, part filename and package name goes through
/// [`sdm_core::safe_path_component`] first. Ordinary names are unchanged.
pub(crate) trait JoinName {
    fn join_name(&self, name: &str) -> PathBuf;
}

impl JoinName for Path {
    fn join_name(&self, name: &str) -> PathBuf {
        self.join(sdm_core::safe_path_component(name))
    }
}

impl JoinName for PathBuf {
    fn join_name(&self, name: &str) -> PathBuf {
        self.as_path().join_name(name)
    }
}

/// Reduces a package's own name, every item's output filename and every
/// component's part filename to a single safe path component, where they
/// first enter the engine (`add`, state restored from disk).
pub(crate) fn sanitize_package_names(package: &mut sdm_core::DownloadPackage) {
    package.name = sdm_core::safe_path_component(&package.name);
    for item in &mut package.items {
        item.output_filename = sdm_core::safe_path_component(&item.output_filename);
        for component in &mut item.components {
            component.part_filename = sdm_core::safe_path_component(&component.part_filename);
        }
    }
}
