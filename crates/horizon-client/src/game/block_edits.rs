//! A server mod's bulk block edit (`native_edit_blocks`): its palette and
//! filters read as commands read blocks, its heights read, and its runs made
//! at once through `World::edit_runs`, with what `fill ... strict` does for
//! the block entities and the watched blocks of each block changed.
use super::Game;
use super::native_world_columns::NativeColumnBlocks as ColumnBlocks;
use super::native_world_edit::{NativeBlockEdit as BlockEdit, NativeHeightmap as Heightmap};
use horizon_client::{
    block_edit::Run,
    chunk::MIN_Y,
    command::{Reader, args::BlockInput, block},
    containers::BlockEntity,
    fast_hash::FxHashMap,
    world::World,
};
use horizon_world::{
    heightmap::HeightmapType, registry::BlockStateId, worldgen::terrain::TerrainBlockStates,
};
use std::sync::Arc;

/// A change `World::edit_runs` reported: position, previous and new state.
type Tracked = ((i32, i32, i32), u32, u32);

impl Game<'_> {
    /// Make a mod's block edit; how many blocks changed, or why none did.
    /// Every palette entry, filter and run is checked before anything
    /// changes: an entry or filter that doesn't read, one with NBT (`{...}`,
    /// refused: the bulk path sets states only), a run naming a palette entry
    /// or filter past the end, or more than 65536 filters, and nothing is
    /// made.
    pub(super) fn edit_blocks(&mut self, edit: &BlockEdit) -> Result<u64, String> {
        let parse = |name: &str, testing: bool| -> Result<BlockInput, String> {
            let mut reader = Reader::new(name);
            let b = block::parse(&mut reader, &self.ui.registries, testing)
                .map_err(|e| format!("{name:?}: {}", e.message.plain()))?;
            if reader.can_read() {
                return Err(format!("{name:?}: more after the block"));
            }
            if b.nbt.is_some() {
                return Err(format!("{name:?}: block NBT is not supported here"));
            }
            Ok(b)
        };
        let mut palette = Vec::with_capacity(edit.palette.len());
        for name in &edit.palette {
            let b = parse(name, false)?;
            let state = self
                .block_state(&b)
                .map_err(|e| format!("{name:?}: {}", e.message.plain()))?;
            palette.push(state.0);
        }
        let filters = edit
            .filters
            .iter()
            .map(|f| parse(f, true))
            .collect::<Result<Vec<_>, _>>()?;
        let runs = resolve_runs(edit, &palette, filters.len(), |h, x, z| {
            self.edit_height(h, x, z)
        })?;
        // (what the registry, the tags and the facts tell of a state, beside the world being changed)
        let registry = self.pack.assets.registry;
        // (the tags as `block_matches` reads them for commands)
        let world_items = match &self.world_tables {
            Some(t) => &t.items,
            None => self.res.items(),
        };
        let tags = [&self.res.items().block_tags, &world_items.block_tags];
        let facts = self.res.facts();
        let watched = std::mem::take(&mut self.world.watched);
        let out = self.world.edit_runs(
            &runs,
            |f, s| {
                super::commands::block_input_matches(
                    registry,
                    tags,
                    &filters[f as usize],
                    BlockStateId(s),
                )
            },
            |s| {
                let f = facts.get(BlockStateId(s));
                watched.get(s as usize).copied().unwrap_or(false)
                    || f.block_entity
                    || BlockEntity::for_block(&f.name).is_some()
            },
        );
        self.world.watched = watched;
        // The block entities the new blocks bring, made once a block, as `native_set_block` makes them (a mod
        // block's own, API 15, included).
        let name = |s: u32| self.res.facts().get(BlockStateId(s)).name.clone();
        let wanted: Vec<Arc<str>> = out
            .tracked
            .iter()
            .filter(|&&(_, old, new)| name(old) != name(new))
            .map(|&(_, _, new)| name(new))
            .collect();
        let mut made: FxHashMap<Arc<str>, Option<BlockEntity>> = FxHashMap::default();
        for n in wanted {
            if let std::collections::hash_map::Entry::Vacant(slot) = made.entry(n.clone()) {
                slot.insert(self.new_block_entity(&n));
            }
        }
        let facts = self.res.facts();
        let removed = replace_block_entities(
            &mut self.world,
            &out.tracked,
            |s| facts.get(BlockStateId(s)).name.clone(),
            |n| made.get(n).cloned().flatten(),
        );
        for pos in removed {
            self.jukebox_removed(pos);
        }
        Ok(out.changed)
    }

    /// `world.surface-height`: each column's height on `h` as `edit_height` reads it, none where its chunk is not
    /// loaded. (A chunk read is looked up once for a run of columns in it.)
    pub(super) fn surface_heights(&self, columns: &[(i32, i32)], h: Heightmap) -> Vec<Option<i32>> {
        let mut last: Option<((i32, i32), bool)> = None;
        columns
            .iter()
            .map(|&(x, z)| {
                let key = (x >> 4, z >> 4);
                let loaded = match last {
                    Some((k, l)) if k == key => l,
                    _ => {
                        let l = self.world.chunks.get(&key).is_some();
                        last = Some((key, l));
                        l
                    }
                };
                loaded.then(|| self.edit_height(h, x, z))
            })
            .collect()
    }

    /// `world.read-columns`: each column's height on `h` (none where its chunk is not loaded) and its blocks from `below`
    /// under it to `above` - 1 over it, bottom first (air outside the world's height and where it is not loaded), the
    /// blocks as indices into the names seen (the tag is the caller's).
    pub(super) fn read_columns(
        &self,
        columns: &[(i32, i32)],
        h: Heightmap,
        below: u32,
        above: u32,
    ) -> ColumnBlocks {
        let bases = self.surface_heights(columns, h);
        let per = (below + above) as usize;
        let mut blocks = Vec::with_capacity(columns.len() * per);
        let mut palette: Vec<String> = Vec::new();
        // (a state's index into the names, worked out once a state)
        let mut seen: FxHashMap<u32, u16> = FxHashMap::default();
        let facts = self.res.facts();
        let mut index = |s: BlockStateId| -> u16 {
            *seen.entry(s.0).or_insert_with(|| {
                let name = &facts.get(s).name;
                let i = palette
                    .iter()
                    .position(|n| **n == **name)
                    .unwrap_or_else(|| {
                        palette.push(name.to_string());
                        palette.len() - 1
                    });
                i as u16
            })
        };
        let top = MIN_Y + horizon_client::chunk::HEIGHT - 1;
        let biomes = self.res.biomes();
        let mut tints = Vec::with_capacity(columns.len() * 3);
        for (&(x, z), base) in columns.iter().zip(&bases) {
            // (the biome tints at the column's top, as the block there is tinted)
            match base.and_then(|b| {
                self.world
                    .chunks
                    .get(&(x >> 4, z >> 4))
                    .map(|c| c.biome(x, b - 1, z))
            }) {
                Some(id) => {
                    let b = biomes.get(id);
                    tints.extend([
                        biomes.grass(id, f64::from(x), f64::from(z)),
                        b.foliage,
                        b.water,
                    ]);
                }
                None => tints.extend([0, 0, 0]),
            }
            match base {
                Some(b) => {
                    let y0 = b - below as i32;
                    for y in y0..y0 + per as i32 {
                        let s = if (MIN_Y..=top).contains(&y) {
                            self.world.block(x, y, z)
                        } else {
                            self.world.air
                        };
                        blocks.push(index(s));
                    }
                }
                None => {
                    let air = index(self.world.air);
                    blocks.extend(std::iter::repeat_n(air, per));
                }
            }
        }
        ColumnBlocks {
            tag: 0,
            palette,
            bases,
            blocks,
            tints,
        }
    }

    /// One above the highest block of column (x, z) that counts on heightmap `h`, or the bottom of the column (also
    /// where it is not loaded).
    fn edit_height(&self, h: Heightmap, x: i32, z: i32) -> i32 {
        if h == Heightmap::WorldSurface {
            return self
                .world
                .world_surface(x, z, |s| self.res.facts().is_air(s));
        }
        let properties = &self.pack.assets.properties;
        let counts = |t: HeightmapType, s: BlockStateId| t.is_opaque(properties.heightmap_state(s));
        // (as `rain_height` reads it: both others count only blocks that block motion, so they are at or under it)
        let top = self
            .world
            .motion_blocking(x, z, |s| counts(HeightmapType::MotionBlocking, s));
        let kind = match h {
            Heightmap::MotionBlockingNoLeaves => HeightmapType::MotionBlockingNoLeaves,
            Heightmap::OceanFloor => HeightmapType::OceanFloor,
            _ => return top,
        };
        let bottom = self
            .world
            .chunks
            .get(&(x >> 4, z >> 4))
            .map_or(self.world.layout().min_y(), |c| c.min_y());
        let mut y = top;
        while y > bottom && !counts(kind, self.world.block(x, y - 1, z)) {
            y -= 1;
        }
        y
    }
}

/// `edit`'s runs as `World::edit_runs` takes them: each palette entry `block`
/// as its state in `palette`, each `y` counted from `height(surface, x, z)`
/// when the run names a heightmap (read before any change, once a column).
/// Err, with nothing resolved, for a run naming a palette entry or a filter
/// past the end, or for more filters than a run can name (65536).
fn resolve_runs(
    edit: &BlockEdit,
    palette: &[u32],
    filters: usize,
    mut height: impl FnMut(Heightmap, i32, i32) -> i32,
) -> Result<Vec<Run>, String> {
    if filters > usize::from(u16::MAX) + 1 {
        return Err(format!("{filters} filters: 65536 at most"));
    }
    let mut heights: FxHashMap<(i32, i32, u8), i32> = FxHashMap::default();
    let mut runs = Vec::with_capacity(edit.runs.len());
    for (i, r) in edit.runs.iter().enumerate() {
        let Some(&state) = usize::try_from(r.block).ok().and_then(|b| palette.get(b)) else {
            return Err(format!(
                "run {i}: palette entry {} of {}",
                r.block,
                palette.len()
            ));
        };
        let only = match r.only {
            None => None,
            Some(f) if (f as usize) < filters => Some(f as u16),
            Some(f) => return Err(format!("run {i}: filter {f} of {filters}")),
        };
        if r.height == 0 {
            continue;
        }
        let base = match r.surface {
            None => i64::from(r.y),
            Some(h) => {
                let top = *heights
                    .entry((r.x, r.z, h as u8))
                    .or_insert_with(|| height(h, r.x, r.z));
                i64::from(top) + i64::from(r.y)
            }
        };
        let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        runs.push(Run {
            x: r.x,
            z: r.z,
            y0: clamp(base),
            y1: clamp(base + i64::from(r.height) - 1),
            state,
            only,
        });
    }
    Ok(runs)
}

/// What `fill ... strict` does for the block entities and the watched blocks
/// of `tracked`'s changes: the watched writes recorded, the block entities
/// told of their new state, and where a different block replaced the block,
/// its block entity removed (as `place_state` does) and `new_entity(name)`'s,
/// if any, put in its place. Returns the positions whose block was replaced
/// (a jukebox there stops).
fn replace_block_entities(
    world: &mut World,
    tracked: &[Tracked],
    name: impl Fn(u32) -> Arc<str>,
    new_entity: impl Fn(&str) -> Option<BlockEntity>,
) -> Vec<[i32; 3]> {
    let mut replaced = Vec::new();
    for &((x, y, z), old, new) in tracked {
        let watches = |id: u32| world.watched.get(id as usize).copied().unwrap_or(false);
        if watches(old) || watches(new) {
            world.watched_writes.push(((x, y, z), old, new));
        }
        world.block_entities.state_changed((x, y, z), old, new);
        let (was, now) = (name(old), name(new));
        if was != now {
            replaced.push([x, y, z]);
            world.block_entities.remove(&(x, y, z));
            if let Some(entity) = new_entity(&now) {
                world.block_entities.insert((x, y, z), entity);
            }
        }
    }
    replaced
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::native_world_edit::NativeBlockRun as BlockRun;
    use horizon_client::chunk::Chunk;

    fn run(block: u32, only: Option<u32>) -> BlockRun {
        BlockRun {
            x: 1,
            z: 2,
            y: 3,
            height: 4,
            surface: None,
            block,
            only,
        }
    }
    fn edit(runs: Vec<BlockRun>) -> BlockEdit {
        BlockEdit {
            palette: vec!["stone".into(), "air".into()],
            filters: vec!["#dirt".into()],
            runs,
        }
    }

    #[test]
    fn runs_naming_entries_past_the_end_are_refused_whole() {
        let palette = [1, 0];
        let ok = resolve_runs(&edit(vec![run(1, Some(0))]), &palette, 1, |_, _, _| 0).unwrap();
        assert_eq!(
            ok,
            [Run {
                x: 1,
                z: 2,
                y0: 3,
                y1: 6,
                state: 0,
                only: Some(0),
            }]
        );
        // a bad run after a good one: nothing at all, and no height read
        let mut asked = 0;
        let mut bad = |runs| {
            resolve_runs(&edit(runs), &palette, 1, |_, _, _| {
                asked += 1;
                0
            })
        };
        assert!(bad(vec![run(0, None), run(2, None)]).is_err());
        assert!(bad(vec![run(0, None), run(u32::MAX, None)]).is_err());
        assert!(bad(vec![run(0, Some(1))]).is_err());
        assert!(bad(vec![run(0, Some(u32::MAX))]).is_err());
        assert_eq!(asked, 0);
        // more filters than a run's u16 names
        assert!(resolve_runs(&edit(vec![]), &palette, 65_537, |_, _, _| 0).is_err());
        assert!(resolve_runs(&edit(vec![]), &palette, 65_536, |_, _, _| 0).is_ok());
    }

    #[test]
    fn a_run_counts_from_its_heightmap_read_once_a_column() {
        let mut r = run(0, None);
        r.surface = Some(Heightmap::OceanFloor);
        r.y = -2;
        let mut asked = Vec::new();
        let runs = resolve_runs(&edit(vec![r, r]), &[7], 0, |h, x, z| {
            asked.push((h, x, z));
            70
        })
        .unwrap();
        assert_eq!(asked, [(Heightmap::OceanFloor, 1, 2)]);
        assert_eq!((runs[0].y0, runs[0].y1, runs[0].state), (68, 71, 7));
        assert_eq!(runs[1], runs[0]);
        // a run far past the world's height clamps rather than wraps
        let mut high = run(0, None);
        high.y = i32::MAX;
        high.height = u32::MAX;
        let runs = resolve_runs(&edit(vec![high]), &[7], 0, |_, _, _| 0).unwrap();
        assert_eq!((runs[0].y0, runs[0].y1), (i32::MAX, i32::MAX));
    }

    fn world() -> World {
        let mut w = World::new(BlockStateId(0), 0);
        w.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        w
    }
    fn names(s: u32) -> Arc<str> {
        match s {
            0 => "minecraft:air",
            1 => "minecraft:stone",
            2 => "minecraft:chest",
            _ => "machines:press",
        }
        .into()
    }

    #[test]
    fn a_chest_replaced_by_stone_loses_its_block_entity() {
        let mut w = world();
        w.set(1, 5, 1, BlockStateId(2)).unwrap();
        let chest = BlockEntity::for_block("minecraft:chest").expect("a chest's");
        w.block_entities.insert((1, 5, 1), chest);
        let out = w.edit_runs(
            &[Run {
                x: 1,
                z: 1,
                y0: 4,
                y1: 6,
                state: 1,
                only: None,
            }],
            |_, _| true,
            |s| s == 2,
        );
        assert_eq!(out.tracked, [((1, 5, 1), 2, 1)]);
        let replaced = replace_block_entities(&mut w, &out.tracked, names, BlockEntity::for_block);
        assert_eq!(replaced, [[1, 5, 1]]);
        assert!(w.block_entities.get(&(1, 5, 1)).is_none());
        assert_eq!(w.block(1, 5, 1).0, 1);
    }

    #[test]
    fn a_new_block_gets_the_block_entity_made_for_it() {
        let mut w = world();
        let out = w.edit_runs(
            &[Run {
                x: 3,
                z: 3,
                y0: 10,
                y1: 10,
                state: 3,
                only: None,
            }],
            |_, _| true,
            |s| s == 3,
        );
        // a mod block's, as `Game::new_block_entity` makes it
        let made = |n: &str| {
            (n == "machines:press").then(|| {
                BlockEntity::Mod(Box::new(horizon_client::containers::ModBlockEntity {
                    id: "machines:press".into(),
                    ..Default::default()
                }))
            })
        };
        replace_block_entities(&mut w, &out.tracked, names, made);
        assert!(matches!(
            w.block_entities.get(&(3, 10, 3)),
            Some(BlockEntity::Mod(e)) if e.id == "machines:press"
        ));
    }
}
