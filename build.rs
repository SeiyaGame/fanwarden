//! The web UI (`web/dist`) is embedded in the binary. When it has not been built yet,
//! a placeholder page is created so that `cargo build` and `cargo test` still work.

use std::path::Path;

fn main() {
    let dist = Path::new("web/dist");
    println!("cargo:rerun-if-changed=web/dist");
    if !dist.join("index.html").exists() {
        std::fs::create_dir_all(dist).expect("cannot create web/dist");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>fan-control</title><p>Web UI not built: run <code>npm ci &amp;&amp; npm run build</code> in <code>web/</code>.</p>",
        )
        .expect("cannot write placeholder UI");
    }
}
