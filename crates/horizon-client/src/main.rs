//! Horizon: native Rust client for Minecraft Java Edition 26.3 gameplay.
#![forbid(unsafe_code)]

#[global_allocator]
static ALLOCATOR: horizon_threads::Allocator = horizon_threads::Allocator;

/// The mod API level this game serves, kept in the program for the launcher
/// to read without running it (`horizon_mods::api_level::MARK`). `main`
/// reads it, so the linker keeps it.
static MOD_API_LEVEL: [u8; 27] = horizon_mods::api_level::MARK;

// The game's unit tests, in this test build only (lib.rs); nothing here runs the game.
#[cfg(test)]
#[allow(dead_code, unused_imports)]
mod game;

fn main() {
    std::hint::black_box(&MOD_API_LEVEL);
    horizon_client::entry::main();
}
