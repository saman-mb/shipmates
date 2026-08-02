// Canonical sources compiled into the binary at build time by `build.rs`.
//
// The generator writes `$OUT_DIR/embedded_sources.rs` defining
// `embedded_sources() -> &'static [(&'static str, &'static str)]` — one
// `(relative_path, content)` pair per `crew/*.md` and `commands/*.md` file,
// via `include_str!`. A `brew`/`cargo`-installed `shipmates` has no checkout
// to read these from at runtime, so install uses this payload.
include!(concat!(env!("OUT_DIR"), "/embedded_sources.rs"));
