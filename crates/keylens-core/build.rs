fn main() {
    // Rebuild when data files change so include_dir! bundles the latest built-in data
    println!("cargo:rerun-if-changed=../../data/apps");
}
