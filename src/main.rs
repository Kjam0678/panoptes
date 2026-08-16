#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod catalog;
mod dummy_items;
mod game_settings;
mod icons;
mod loadout;
mod model;
mod paths;
mod settings;
mod storage;

fn main() -> eframe::Result {
    app::run()
}
