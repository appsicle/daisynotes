//! Link the vendored Sparkle.framework (the macOS auto-updater) into the app
//! binary. The classes are reached dynamically through the Objective-C runtime
//! (see `src/updater.rs`), so linking only needs to add the load command and
//! the rpaths that resolve the framework at run time:
//!
//! - `@executable_path/../Frameworks` — where `package.sh` embeds it in the
//!   shipped `DaisyNotes.app` bundle.
//! - the vendored `third_party/Sparkle` path — so a plain `cargo run` during
//!   development finds it without a bundle.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let manifest = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );
    let sparkle = manifest.join("../../third_party/Sparkle");
    let sparkle = sparkle.canonicalize().unwrap_or(sparkle);

    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rustc-link-search=framework={}",
        sparkle.display()
    );
    // `-needed_framework` (not `-framework`): the classes are reached only
    // through the Objective-C runtime, so nothing references a Sparkle symbol
    // at link time. Without `needed`, the linker's dead-strip could drop the
    // load command and Sparkle would never load at run time.
    println!("cargo:rustc-link-arg=-Wl,-needed_framework,Sparkle");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", sparkle.display());
}
