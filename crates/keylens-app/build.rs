fn main() {
    // 界面文字变化时重新编译，让 rust_i18n::i18n! 读到最新的 locales。
    println!("cargo:rerun-if-changed=locales");
}
