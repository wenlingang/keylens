fn main() {
    // Rebuild when UI strings change so rust_i18n::i18n! picks up the latest locales.
    println!("cargo:rerun-if-changed=locales");
}
