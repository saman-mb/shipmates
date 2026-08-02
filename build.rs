use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR unset");
    let root = Path::new(&manifest);

    println!("cargo:rerun-if-changed=crew");
    println!("cargo:rerun-if-changed=commands");

    let mut entries: Vec<(String, String)> = Vec::new();
    for dir in ["crew", "commands"] {
        let base = root.join(dir);
        if !base.is_dir() {
            continue;
        }
        let mut files: Vec<_> = fs::read_dir(&base)
            .expect("read canonical dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            .collect();
        files.sort_by_key(|e| e.path());
        for f in files {
            let path = f.path();
            let rel = format!("{}/{}", dir, path.file_name().unwrap().to_string_lossy());
            entries.push((rel, path.to_string_lossy().into_owned()));
        }
    }

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR unset");
    let mut out = String::new();
    out.push_str("pub fn embedded_sources() -> &'static [(&'static str, &'static str)] {\n");
    out.push_str("    &[\n");
    for (rel, path) in &entries {
        out.push_str(&format!("        ({:?}, include_str!({:?})),\n", rel, path));
    }
    out.push_str("    ]\n");
    out.push_str("}\n");

    fs::write(Path::new(&out_dir).join("embedded_sources.rs"), out).expect("write embedded sources");
}
