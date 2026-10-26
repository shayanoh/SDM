fn main() {
    // tauri-build embeds `icons/icon.ico` into the Windows executable but does
    // not tell cargo to watch it, so a changed icon kept the old one until the
    // next clean build. Watching the folder makes cargo rerun this script.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
