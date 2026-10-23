//! What "Remove and Delete Files" is about to put in the Trash: the real files
//! on disk, not the sizes the engine estimates. Port of
//! `MainWindowView.deletionInfo(for:)`.

use std::collections::BTreeSet;
use std::path::Path;

/// For each item `(output file name, part file names)`, sums the on-disk size
/// of every file that exists in `folder` — the output, each part, and the
/// `.incomplete` sibling of each — and lists their names, sorted and without
/// repeats. A missing file or folder simply contributes nothing.
pub fn describe(folder: &Path, files: &[(String, Vec<String>)]) -> (f64, Vec<String>) {
    let mut names = BTreeSet::new();
    let mut total: u64 = 0;
    for (output, parts) in files {
        for base in std::iter::once(output).chain(parts) {
            for name in [base.clone(), format!("{base}.incomplete")] {
                if names.contains(&name) || !is_plain_file_name(&name) {
                    continue;
                }
                if let Ok(metadata) = std::fs::metadata(folder.join(&name))
                    && metadata.is_file()
                {
                    total += metadata.len();
                    names.insert(name);
                }
            }
        }
    }
    (total as f64, names.into_iter().collect())
}

/// A bare file name: never a path, so a name can not point outside `folder`.
fn is_plain_file_name(name: &str) -> bool {
    Path::new(name).file_name().is_some_and(|f| f == name)
}
