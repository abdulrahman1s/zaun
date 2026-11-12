//! Fixtures: PE builders (`pe.rs`), game and policy samples, `HostFacts` presets, directory
//! trees. Each file is added with the milestone that needs it (MODULES §11); this module only
//! hosts small shared samples for now.

pub mod games;
pub mod host_trees;
pub mod hosts;
pub mod images;
pub mod policies;
pub mod lnk;
pub mod pe;
pub mod runtime_trees;

use std::path::Path;

/// Write the directory tree used by walk and removal tests into `fs`:
///
/// ```text
/// <root>/a.txt
/// <root>/sub/b.txt
/// <root>/sub/deep/c.txt
/// <root>/link_out -> /etc
/// <root>/loop -> .
/// ```
pub fn basic_tree(fs: &crate::MemFs, root: &Path) {
    fs.file(root.join("a.txt"), b"hello")
        .file(root.join("sub/b.txt"), b"b")
        .file(root.join("sub/deep/c.txt"), b"c")
        .link(root.join("link_out"), "/etc")
        .link(root.join("loop"), ".");
}
