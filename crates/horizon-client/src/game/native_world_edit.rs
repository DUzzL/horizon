//! Bulk block edits for server mods: the WebAssembly mod API's `world.edit-blocks` from the Stink-O/horizon fork,
//! as a helper on the level being ticked, as `native_set_block` is.
use super::Game;

/// `world.heightmap`: which of a column's heights a run's `y` counts from.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeHeightmap {
    WorldSurface,
    MotionBlocking,
    MotionBlockingNoLeaves,
    OceanFloor,
}
/// `world.block-run`: `height` blocks up a column from `y` (from the column's `surface` height when one is
/// given), set to palette entry `block`, only where filter `only` matches when one is given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NativeBlockRun {
    pub(crate) x: i32,
    pub(crate) z: i32,
    pub(crate) y: i32,
    pub(crate) height: u32,
    pub(crate) surface: Option<NativeHeightmap>,
    pub(crate) block: u32,
    pub(crate) only: Option<u32>,
}
/// `world.block-edit`: block states and block predicates as commands write them, and the runs naming them.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct NativeBlockEdit {
    pub(crate) palette: Vec<String>,
    pub(crate) filters: Vec<String>,
    pub(crate) runs: Vec<NativeBlockRun>,
}

#[allow(dead_code)]
impl Game<'_> {
    /// `world.edit-blocks`, for a server mod: the runs made at once, straight into the chunks of the authoritative
    /// level `dimension` (the one being ticked, as for `native_set_block`), with what `fill ... strict` does for
    /// block entities; the chunks changed go to the players whole. The blocks changed, or why the edit was left
    /// out: another dimension, a client of a server, or a palette entry or filter that doesn't read. API @API:native_edit_blocks@.
    pub(super) fn native_edit_blocks(
        &mut self,
        dimension: &str,
        edit: &NativeBlockEdit,
    ) -> Result<u64, String> {
        if !self.native_level_open(dimension) {
            return Err(format!("{dimension} is not the level being ticked"));
        }
        let started = std::time::Instant::now();
        let changed = self.edit_blocks(edit)?;
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        if self.options.spikes.is_some_and(|threshold| ms > threshold) {
            log::info!(
                "Horizon: slow mod block edit {ms:.1} ms ({} runs, {changed} blocks)",
                edit.runs.len()
            );
        }
        Ok(changed)
    }
    /// Whether `dimension` is the authoritative level being ticked: where a server mod's world helpers act.
    pub(super) fn native_level_open(&self, dimension: &str) -> bool {
        (self.serving.is_some() || self.options.dedicated.is_some()) && dimension == self.dim.id()
    }
}
