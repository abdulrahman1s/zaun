//! Packs Twemoji's SVGs (the Chat page's emoji pictures, ADR-0033) into one compressed blob
//! in `OUT_DIR`, with an index, so the app ships them deflated instead of as ~10 MB of SVG
//! text (`twemoji-assets` is a build-dependency only). Every emoji the `emojis` crate knows,
//! in every skin tone, is looked up the way the app looks one up (as written, then without
//! U+FE0F); the pictures are deflated in runs of 32, so the app inflates a run on first use.
//! The output depends only on the two crates' versions: the build is reproducible and needs
//! no network.

#![allow(clippy::expect_used, clippy::disallowed_methods)] // A build script: it writes its output and stops on failure.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;

use twemoji_assets::svg::SvgTwemojiAsset;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    // The picture's key (the asset's own emoji string) → its SVG.
    let mut svgs: BTreeMap<&'static str, &'static str> = BTreeMap::new();
    let mut add = |s: &str| {
        let bare: String = s.chars().filter(|&c| c != '\u{FE0F}').collect();
        for candidate in [s, bare.as_str()] {
            if let Some(asset) = SvgTwemojiAsset::from_emoji(candidate) {
                svgs.insert(asset.emoji, asset.data.0);
            }
        }
    };
    for e in emojis::iter() {
        add(e.as_str());
        if let Some(tones) = e.skin_tones() {
            for t in tones {
                add(t.as_str());
            }
        }
    }

    // Pictures in runs of CHUNK (in key order, so a face's skin tones sit together), each
    // run deflated as one: about a fifth of the SVG text, and one picture costs inflating
    // its run (~80 KB) the first time.
    const CHUNK: usize = 32;
    let entries: Vec<(&str, &str)> = svgs.into_iter().collect();
    let mut blob = Vec::new();
    let mut chunks = String::from(
        "/// Runs of pictures: (offset, length) of each deflated run in the blob.\n\
         pub static CHUNKS: &[(u32, u32)] = &[\n",
    );
    let mut index = String::from(
        "/// Twemoji pictures: (key, run, offset, length) inside the inflated run, sorted by key.\n\
         pub static INDEX: &[(&str, u32, u32, u32)] = &[\n",
    );
    for (n, run) in entries.chunks(CHUNK).enumerate() {
        let mut raw = Vec::new();
        for (key, svg) in run {
            let escaped: String = key
                .chars()
                .map(|c| format!("\\u{{{:x}}}", c as u32))
                .collect();
            writeln!(
                index,
                "    (\"{escaped}\", {n}, {}, {}),",
                raw.len(),
                svg.len()
            )
            .expect("index");
            raw.extend_from_slice(svg.as_bytes());
        }
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(&raw).expect("deflate");
        let packed = enc.finish().expect("deflate");
        writeln!(chunks, "    ({}, {}),", blob.len(), packed.len()).expect("chunks");
        blob.extend_from_slice(&packed);
    }
    index.push_str("];\n");
    chunks.push_str("];\n");
    index.push_str(&chunks);

    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("twemoji.bin"), &blob).expect("write the blob");
    std::fs::write(out.join("twemoji_index.rs"), index).expect("write the index");
}
