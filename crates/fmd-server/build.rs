//! Rebuilds the crate when the web UI build (`web/build`, embedded by rust-embed) appears or
//! changes; rust-embed alone does not notice a folder that was missing at the last build.

fn main() {
    println!("cargo:rerun-if-changed=../../web/build");
}
