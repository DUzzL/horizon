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
    // Blocks with identical pixels share a layer (and the shader pairs by layer): a shared
    // base would glow on every block that shares it, so it gets no overlay.
    let mut sharing: BTreeMap<u32, usize> = BTreeMap::new();
    for layer in sprites.layers.values() {
        *sharing.entry(*layer).or_default() += 1;
    }
    let bases: Vec<String> = sprites
        .layers
        .iter()
        .filter(|(name, layer)| {
            sharing[layer] == 1
                && name
                    .split_once(':')
                    .is_some_and(|(_, p)| p.starts_with("block/") && !p.ends_with(&suffix))
        })
        .map(|(name, _)| name.clone())
        .collect();
    let mut pairs = Vec::new();
    for base in bases {
        let overlay = format!("{base}{suffix}");
        let file = crate::atlas_sources::texture_path(root, &overlay, "textures");
        if !std::fs::metadata(&file).is_ok_and(|m| m.is_file()) {
            continue;
        }
        if pairs.len() == MAX {
            log::warn!(
                "Horizon: more than {MAX} emissive block overlays: {overlay} and later skipped"
            );
            break;
        }
        if std::fs::metadata(&file).is_ok_and(|m| m.len() > 1 << 20) {
            log::warn!("Horizon: emissive overlay {overlay} is over 1 MiB, skipped");
            continue;
        }
        // A file that doesn't load would glow as the missing texture: no pair instead.
        match sprites.try_insert(root, &overlay) {
            Ok(_) => pairs.push((base, overlay)),
            Err(e) => log::warn!("Horizon: emissive overlay {overlay} skipped: {e}"),
        }
    }
    pairs
}

/// The overlay layer's flag (its high bit) for an OptiFine overlay, which draws cutout: opaque
/// where its alpha is at least 0.1 (OptiFine renders SOLID textures' overlays as CUTOUT).
pub const CUTOUT: u32 = 1 << 31;

/// The frame's overlay table: native mods' pairs (at most [`NATIVE`] requested), then the packs'
/// OptiFine pairs flagged [`CUTOUT`], one per base layer (native first), sorted by base layer
/// for the shader's early exit, at most [`MAX`].
pub fn table<'a>(
    layers: &BTreeMap<String, u32>,
    native: impl IntoIterator<Item = (&'a str, &'a str)>,
    optifine: &'a [(String, String)],
) -> Vec<[u32; 4]> {
    let mut by_base: BTreeMap<u32, [u32; 4]> = BTreeMap::new();
    for pair in mappings(layers, native.into_iter().take(NATIVE)) {
        by_base.insert(pair[0], pair);
    }
    let optifine = optifine.iter().map(|(b, o)| (b.as_str(), o.as_str()));
    for [b, o, z, w] in mappings(layers, optifine) {
        by_base.entry(b).or_insert([b, o | CUTOUT, z, w]);
    }
    by_base.into_values().take(MAX).collect()
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

    /// A 16x16 image of one colour, `shade` its red (images of different shades never share a
    /// sprite layer).
    fn png(path: &Path, shade: u8) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        let mut encoder = png::Encoder::new(file, 16, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let pixels: Vec<u8> = (0..16 * 16).flat_map(|_| [shade, 9, 9, 255]).collect();
        writer.write_image_data(&pixels).unwrap();
    }

    #[test]
    fn optifine_overlays_pair_with_their_loaded_blocks() {
        let root =
            std::env::temp_dir().join(format!("horizon-block-emissive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let block = root.join("assets/minecraft/textures/block");
        let names = [
            "stone",
            "stone_e",
            "dirt",
            "glowstone_e",
            "deepslate",
            "deepslate_glow",
            "andesite",
            "granite",
        ];
        for (i, name) in names.iter().enumerate() {
            png(&block.join(format!("{name}.png")), i as u8 * 20);
        }
        // Same pixels as andesite: the two share a layer, so neither gets an overlay.
        png(&block.join("diorite.png"), 6 * 20);
        png(&block.join("diorite_e.png"), 200);
        // A broken overlay: skipped, not drawn as the missing texture.
        std::fs::write(block.join("granite_e.png"), b"not a png").unwrap();
        png(&root.join("assets/minecraft/textures/item/stick.png"), 210);
        png(
            &root.join("assets/minecraft/textures/item/stick_e.png"),
            220,
        );
        let mut sprites = Sprites::default();
        for name in [
            "stone",
            "dirt",
            "deepslate",
            "andesite",
            "diorite",
            "granite",
        ] {
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
            )]
        );
        assert!(sprites.layers.contains_key("minecraft:block/stone_e"));
        assert!(!sprites.layers.contains_key("minecraft:block/granite_e"));
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
    fn the_table_keeps_native_pairs_first_and_flags_optifine_cutout() {
        let layers: BTreeMap<String, u32> =
            [("a", 5), ("a_e", 6), ("b", 2), ("b_e", 3), ("native", 9)]
                .map(|(n, l)| (n.to_owned(), l))
                .into();
        let optifine = [
            ("a".to_owned(), "a_e".to_owned()),
            ("b".to_owned(), "b_e".to_owned()),
        ];
        // A native mod pairs `a` too: its pair wins; `b` stays the pack's, flagged.
        let t = table(&layers, [("a", "native")], &optifine);
        assert_eq!(
            t,
            [[2, 3 | CUTOUT, 0, 0], [5, 9, 0, 0]],
            "sorted by base layer"
        );
        // Native mods keep their 32.
        let many: Vec<(&str, &str)> = (0..40).map(|_| ("b", "native")).collect();
        assert_eq!(table(&layers, many, &[]), [[2, 9, 0, 0]]);
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
