//! Bundles the icons into the binary as a GResource, and fixes the link order.

use std::fmt::Write;

fn main() {
    // gtk4-layer-shell works by intercepting libwayland-client calls; it must come first
    // in link order, now that the wayland-client crate links libwayland-client too.
    println!("cargo:rustc-link-lib=gtk4-layer-shell");
    println!("cargo:rustc-link-lib=wayland-client");

    let dir = "icons/scalable/status";
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.ends_with(".svg"))
        .collect();
    files.sort();

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gresources>\n  <gresource prefix=\"/dev/nshell/icons/scalable/status\">\n",
    );
    for file in &files {
        let _ = writeln!(xml, "    <file preprocess=\"xml-stripblanks\">{file}</file>");
    }
    xml.push_str("  </gresource>\n</gresources>\n");

    let out = std::env::var("OUT_DIR").unwrap_or_default();
    let manifest = format!("{out}/icons.gresource.xml");
    if let Err(err) = std::fs::write(&manifest, xml) {
        panic!("writing {manifest}: {err}");
    }
    println!("cargo:rerun-if-changed={dir}");
    glib_build_tools::compile_resources(&[dir], &manifest, "icons.gresource");
}
