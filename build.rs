use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR unset");
    let root = Path::new(&manifest);

    println!("cargo:rerun-if-changed=crew");
    println!("cargo:rerun-if-changed=commands");
    println!("cargo:rerun-if-changed=docs/COST.md");
    println!("cargo:rerun-if-changed=toolbox");
    println!("cargo:rerun-if-changed=steering");

    let mut entries: Vec<(String, String)> = Vec::new();
    let steering = root.join("steering").join("shipmates.md");
    if steering.is_file() {
        entries.push((
            "steering/shipmates.md".to_string(),
            steering.to_string_lossy().into_owned(),
        ));
    }
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

    // Tools are folders (`toolbox/<name>/tool.md` + bundled assets), not flat
    // files, so they embed recursively and across every extension (`.md`,
    // `.py`, `.ts`, …) — the whole runnable payload rides in the binary.
    let toolbox = root.join("toolbox");
    if toolbox.is_dir() {
        let mut tool_files: Vec<std::path::PathBuf> = Vec::new();
        collect_files(&toolbox, &mut tool_files);
        tool_files.sort();
        for path in tool_files {
            let rel = path
                .strip_prefix(root)
                .expect("toolbox path under root")
                .to_string_lossy()
                .replace('\\', "/");
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

/// Recursively collect every file under `dir` (build-time only; no walkdir dep).
fn collect_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).expect("read embed dir").filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}
