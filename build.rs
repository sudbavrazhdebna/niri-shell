//! Bundles the icons into the binary as a GResource.

use std::fmt::Write;

fn main() {
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
