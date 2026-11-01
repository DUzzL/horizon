//! Column reads for server mods, as helpers on the level being ticked: many columns' heights, and the blocks round
//! them, in one call.
use super::Game;
use super::native_world_edit::NativeHeightmap;

/// What `native_read_columns` read: the blocks round each column's height (`bases`, none where it isn't loaded),
/// as indexes into `palette`, a column after another, and three biome tints a column (grass, foliage, water).
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct NativeColumnBlocks {
    pub(crate) palette: Vec<String>,
    pub(crate) bases: Vec<Option<i32>>,
    pub(crate) blocks: Vec<u16>,
    pub(crate) tints: Vec<u32>,
}

#[allow(dead_code)]
impl Game<'_> {
    /// For a server mod: each column's height by `heightmap` in the authoritative level
    /// `dimension` (the one being ticked), none for a column not loaded; none at all for another dimension or a
    /// client of a server. API @API:native_read_columns@.
    pub(super) fn native_surface_heights(
        &self,
        dimension: &str,
        columns: &[(i32, i32)],
        heightmap: NativeHeightmap,
    ) -> Option<Vec<Option<i32>>> {
        if !self.native_level_open(dimension) {
            return None;
        }
        let started = std::time::Instant::now();
        let out = self.surface_heights(columns, heightmap);
        self.native_read_logged(started, "height query", columns.len());
        Some(out)
    }
    /// For a server mod: `below` blocks under and `above` blocks from each column's height by `heightmap`, as a
    /// palette and indexes, with the biome's grass, foliage and water tints (air outside the column and in columns
    /// not loaded); none for another dimension or a client of a server, and none, reading nothing, when
    /// `below + above` is more than the level's height or the read more than 16,777,216 blocks (`MAX_READ`).
    /// API @API:native_read_columns@.
    pub(super) fn native_read_columns(
        &self,
        dimension: &str,
        columns: &[(i32, i32)],
        heightmap: NativeHeightmap,
        (below, above): (u32, u32),
    ) -> Option<NativeColumnBlocks> {
        if !self.native_level_open(dimension) {
            return None;
        }
        let started = std::time::Instant::now();
        let out = self.read_columns(columns, heightmap, below, above)?;
        self.native_read_logged(started, "column read", columns.len());
        Some(out)
    }
    /// A read's time logged with `HORIZON_SPIKES`, as an edit's is.
    fn native_read_logged(&self, started: std::time::Instant, what: &str, columns: usize) {
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        if self.options.spikes.is_some_and(|threshold| ms > threshold) {
            log::info!("Horizon: slow mod {what} {ms:.1} ms ({columns} columns)");
        }
    }
}
