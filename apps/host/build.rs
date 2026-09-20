fn main() {
    let root = std::path::Path::new("../../catalog/games");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files: Vec<_> = std::fs::read_dir(root)
        .expect("catalog directory")
        .map(|entry| entry.expect("catalog entry").path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("yaml" | "yml")
            )
        })
        .collect();
    files.sort();
    let mut source = String::from("const BUILTIN_CATALOG: &[&str] = &[\n");
    for path in files {
        source.push_str(&format!(
            "{:?},\n",
            std::fs::read_to_string(path).expect("UTF-8 catalog")
        ));
    }
    source.push_str("];\n");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(output.join("catalog.rs"), source).expect("generated catalog");
}
