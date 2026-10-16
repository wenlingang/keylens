//! Installs the app inside a Keylens .dmg over an existing Keylens.app, exactly as the in-app
//! updater does (signature check included), for testing updates by hand.
//! Usage: cargo run -p keylens-macos --example install_update -- <dmg> <app> <version>

#[cfg(target_os = "macos")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dmg, app, version] = args.as_slice() else {
        eprintln!("usage: install_update <dmg> <app> <version>");
        std::process::exit(2);
    };
    match keylens_macos::update::install(dmg.as_ref(), app.as_ref(), version) {
        Ok(()) => println!("installed {dmg} over {app}"),
        Err(e) => {
            eprintln!("install failed: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
