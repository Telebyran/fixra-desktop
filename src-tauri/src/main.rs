// Inget konsolfönster på Windows i release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    fixra_desktop_lib::run()
}
