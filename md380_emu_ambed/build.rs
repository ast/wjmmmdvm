//! Verifies the firmware files needed by `include_bytes!` are present
//! and tells cargo to re-run the build whenever they change.

use std::path::Path;

fn main() {
    for name in ["firmware/D002.032.img", "firmware/d02032-core.img"] {
        if !Path::new(name).exists() {
            eprintln!(
                "\n\n  ERROR: required firmware file missing: {name}\n\n  \
                 See md380_emu_ambed/firmware/README.md, or run `just sync-firmware`\n  \
                 from the workspace root.\n\n"
            );
            std::process::exit(1);
        }
        println!("cargo:rerun-if-changed={name}");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
