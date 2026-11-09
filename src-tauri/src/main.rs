// Cegah jendela konsol tambahan di Windows release (tidak relevan di macOS).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    selisih_app_lib::run()
}
