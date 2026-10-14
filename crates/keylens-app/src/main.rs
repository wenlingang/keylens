fn main() {
    #[cfg(target_os = "macos")]
    keylens_app::run();
    #[cfg(not(target_os = "macos"))]
    eprintln!("Keylens 目前只支持 macOS");
}
