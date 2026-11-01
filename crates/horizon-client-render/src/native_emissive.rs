//! Optional, texture-matched overlays drawn full bright over their base texture in the same
//! terrain fragment: native mods' (`<ns>:emissive/...`) and OptiFine's emissive block textures
//! (`block/stone_e.png` beside `block/stone.png`). No extra draw calls or world light changes.
use crate::sprites::Sprites;
use std::{collections::BTreeMap, path::Path};

/// Overlay pairs the terrain shader reads (`Globals.emissive_layers`, terrain.wgsl).
pub const MAX: usize = 128;
/// Native mods' own `emissive/` textures, as their API documents.
const NATIVE: usize = 32;

/// Load only bounded, explicitly named emissive textures from the active pack.
pub fn load(root: &Path, sprites: &mut Sprites) {
    let mut names: Vec<_> = crate::atlas_sources::list_textures(root)
        .into_iter()
        .filter(|name| {
            name.split_once(':')
                .is_some_and(|(_, p)| p.starts_with("emissive/"))
        })
        .take(NATIVE)
        .collect();
    names.sort();
    for name in names {
        let file = crate::atlas_sources::texture_path(root, &name, "textures");
        if std::fs::metadata(file).is_ok_and(|m| m.len() <= 1 << 20) {
            let _ = sprites.insert(root, &name);
        }
    }
}

/// OptiFine's emissive overlays of the block sprites already loaded (what the terrain draws;
/// items draw from the HUD atlas): for each
/// `<ns>:block/<name>` whose `<name><suffix>.png` sits beside it, the overlay is loaded too and
/// paired with it, first by name, at most [`MAX`]. A pack without such files adds nothing.
pub fn load_optifine(root: &Path, sprites: &mut Sprites) -> Vec<(String, String)> {
    let suffix = horizon_optifine::emissive::suffix(root);
    let bases: Vec<String> = sprites
        .layers
        .keys()
        .filter(|name| {
            name.split_once(':')
                .is_some_and(|(_, p)| p.starts_with("block/") && !p.ends_with(&suffix))
        })
        .cloned()
        .collect();
    let mut pairs = Vec::new();
    for base in bases {
        if pairs.len() == MAX {
            break;
        }
        let overlay = format!("{base}{suffix}");
        let file = crate::atlas_sources::texture_path(root, &overlay, "textures");
        if std::fs::metadata(file).is_ok_and(|m| m.is_file() && m.len() <= 1 << 20)
            && sprites.insert(root, &overlay).is_ok()
        {
            pairs.push((base, overlay));
        }
    }
    pairs
}

/// Only loaded base/overlay pairs are sent to the GPU; sorted for vertex lookup.
pub fn mappings<'a>(
    layers: &BTreeMap<String, u32>,
    requests: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<[u32; 4]> {
    let mut pairs = BTreeMap::new();
    for (base, overlay) in requests.into_iter().take(MAX) {
        if let (Some(&b), Some(&o)) = (layers.get(base), layers.get(overlay)) {
            pairs.entry(b).or_insert(o);
        }
    }
    pairs.into_iter().map(|(b, o)| [b, o, 0, 0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        let mut encoder = png::Encoder::new(file, 16, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255; 16 * 16 * 4]).unwrap();
    }

    #[test]
    fn optifine_overlays_pair_with_their_loaded_blocks() {
        let root =
            std::env::temp_dir().join(format!("horizon-block-emissive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let block = root.join("assets/minecraft/textures/block");
        for name in [
            "stone",
            "stone_e",
            "dirt",
            "glowstone_e",
            "deepslate",
            "deepslate_glow",
        ] {
            png(&block.join(format!("{name}.png")));
        }
        png(&root.join("assets/minecraft/textures/item/stick.png"));
        png(&root.join("assets/minecraft/textures/item/stick_e.png"));
        let mut sprites = Sprites::default();
        for name in ["stone", "dirt", "deepslate"] {
            sprites
                .insert(&root, &format!("minecraft:block/{name}"))
                .unwrap();
        }
        // Items draw from the HUD atlas, not the terrain: no pair.
        sprites.insert(&root, "minecraft:item/stick").unwrap();
        // glowstone isn't loaded: its overlay pairs with nothing.
        let pairs = load_optifine(&root, &mut sprites);
        assert_eq!(
            pairs,
            [(
                "minecraft:block/stone".into(),
                "minecraft:block/stone_e".into()
            ),]
        );
        assert!(sprites.layers.contains_key("minecraft:block/stone_e"));
        // The pack's own suffix.
        let props = root.join("assets/minecraft/optifine/emissive.properties");
        std::fs::create_dir_all(props.parent().unwrap()).unwrap();
        std::fs::write(&props, "suffix.emissive=_glow").unwrap();
        let pairs = load_optifine(&root, &mut sprites);
        assert_eq!(
            pairs,
            [(
                "minecraft:block/deepslate".into(),
                "minecraft:block/deepslate_glow".into()
            )]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn only_existing_pairs_are_mapped() {
        let layers = [
            ("minecraft:block/ore".into(), 3),
            ("mine:emissive/ore".into(), 7),
        ]
        .into();
        assert_eq!(
            mappings(
                &layers,
                [
                    ("minecraft:block/ore", "mine:emissive/ore"),
                    ("missing", "mine:emissive/ore")
                ]
            ),
            vec![[3, 7, 0, 0]]
        );
    }
}
