// Desktop entry point. Mobile targets use `tauri::mobile_entry_point` on
// `amber_shield_lite_lib::run` instead - see lib.rs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    amber_shield_lite_lib::run();
}
