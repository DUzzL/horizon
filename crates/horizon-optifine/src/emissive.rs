//! Emissive textures: a texture's overlay that draws at full brightness, named after it with
//! the pack's emissive suffix (`zombie.png` and `zombie_e.png`).
//!
//! The suffix is `suffix.emissive` in the first of `assets/minecraft/optifine/emissive.properties`,
//! ETF's `assets/minecraft/etf/emissive.properties` and `assets/minecraft/textures/emissive.properties`,
//! and `_e` without one (as ETF reads it). An overlay sits beside its texture, so a random
//! variant's is beside the variant: `optifine/random/entity/zombie/zombie3_e.png` glows over
//! `zombie3`, never over `zombie`.
//!
//! This covers entity textures (mobs and block entities, under `textures/entity/`). Names are
//! those of the entity atlas ([`crate::random_textures`]).
//!
//! Written from OptiFine's `emissive.properties` and ETF's `EMISSIVE_GUIDE.md` and
//! `emissive.properties`; no mod's source was read.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::paths::files_under;
use crate::properties::Properties;
use crate::random_textures::{RandomTextures, atlas_name};

/// The pack's emissive overlays, by the atlas name of the texture they glow over.
#[derive(Debug, Default)]
pub struct Emissive {
    pub suffix: String,
    overlays: HashMap<String, String>,
    /// Overlays outside `textures/` (of random variants): file and atlas name.
    files: Vec<(String, String)>,
}

/// Where the suffix is read, first found first.
const PROPERTIES: [&str; 3] = [
    "assets/minecraft/optifine/emissive.properties",
    "assets/minecraft/etf/emissive.properties",
    "assets/minecraft/textures/emissive.properties",
];

impl Emissive {
    pub fn scan(root: &Path, random: &RandomTextures) -> Self {
        let suffix = PROPERTIES
            .iter()
            .find_map(|p| fs::read_to_string(root.join(p)).ok())
            .and_then(|text| {
                Properties::parse(&text)
                    .get("suffix.emissive")
                    .map(str::to_owned)
            })
            .filter(|s| !s.is_empty() && !s.contains('/'))
            .unwrap_or_else(|| "_e".to_owned());
        let mut out = Self {
            suffix,
            ..Self::default()
        };
        let Ok(namespaces) = fs::read_dir(root.join("assets")) else {
            return out;
        };
        let png = format!("{}.png", out.suffix);
        for ns in namespaces.flatten() {
            let ns = ns.file_name().to_string_lossy().into_owned();
            let prefix = format!("assets/{ns}/");
            for file in files_under(root, &format!("{prefix}textures/entity"), &[png.as_str()]) {
                let overlay = &file[prefix.len()..file.len() - ".png".len()];
                let base = &overlay[..overlay.len() - out.suffix.len()];
                if root.join(format!("{prefix}{base}.png")).is_file() {
                    out.overlays
                        .insert(atlas_name(&ns, base), atlas_name(&ns, overlay));
                }
            }
        }
        for (file, name) in random.files() {
            let Some(stem) = file.strip_suffix(".png") else {
                continue;
            };
            let overlay = format!("{stem}{png}");
            if root.join(&overlay).is_file() {
                let overlay_name = format!("{name}{}", out.suffix);
                out.overlays.insert(name.to_owned(), overlay_name.clone());
                // Those under `textures/` are the atlas's already.
                if !overlay.contains("/textures/") {
                    out.files.push((overlay, overlay_name));
                }
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.overlays.is_empty()
    }

    /// The atlas name of the overlay that glows over `texture`.
    pub fn overlay(&self, texture: &str) -> Option<&str> {
        self.overlays.get(texture).map(String::as_str)
    }

    /// The overlays of random variants: file (from the resource root) and atlas name, for the
    /// entity atlas to pack (it walks `textures/` itself).
    pub fn files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files.iter().map(|(f, n)| (f.as_str(), n.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(name: &str, files: &[&str], suffix: Option<&str>) -> (std::path::PathBuf, Emissive) {
        let root =
            std::env::temp_dir().join(format!("horizon-emissive-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for path in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        if let Some(suffix) = suffix {
            let path = root.join(PROPERTIES[0]);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, format!("suffix.emissive={suffix}\n")).unwrap();
        }
        let e = Emissive::scan(&root, &RandomTextures::scan(&root));
        (root, e)
    }

    const ENTITY: &str = "assets/minecraft/textures/entity";
    const RANDOM: &str = "assets/minecraft/optifine/random/entity";

    #[test]
    fn overlays_beside_their_texture() {
        let (root, e) = pack(
            "beside",
            &[
                &format!("{ENTITY}/zombie/zombie.png"),
                &format!("{ENTITY}/zombie/zombie_e.png"),
                // No base: no overlay.
                &format!("{ENTITY}/zombie/husk_e.png"),
                &format!("{ENTITY}/chest/normal.png"),
                &format!("{ENTITY}/chest/normal_e.png"),
                "assets/mymod/textures/entity/bug/bug.png",
                "assets/mymod/textures/entity/bug/bug_e.png",
                // A variant's overlay glows over the variant only.
                &format!("{RANDOM}/zombie/zombie3.png"),
                &format!("{RANDOM}/zombie/zombie3_e.png"),
                &format!("{RANDOM}/zombie/zombie2.png"),
            ],
            None,
        );
        assert_eq!(e.suffix, "_e");
        assert_eq!(
            e.overlay("entity/zombie/zombie"),
            Some("entity/zombie/zombie_e")
        );
        assert_eq!(e.overlay("entity/zombie/husk"), None);
        assert_eq!(
            e.overlay("entity/chest/normal"),
            Some("entity/chest/normal_e")
        );
        assert_eq!(
            e.overlay("mymod:entity/bug/bug"),
            Some("mymod:entity/bug/bug_e")
        );
        let variant = "minecraft:optifine/random/entity/zombie/zombie3";
        assert_eq!(e.overlay(variant), Some(format!("{variant}_e").as_str()));
        assert_eq!(
            e.overlay("minecraft:optifine/random/entity/zombie/zombie2"),
            None
        );
        assert_eq!(
            e.files().collect::<Vec<_>>(),
            [(
                "assets/minecraft/optifine/random/entity/zombie/zombie3_e.png",
                "minecraft:optifine/random/entity/zombie/zombie3_e"
            )]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_packs_suffix() {
        let (root, e) = pack(
            "suffix",
            &[
                &format!("{ENTITY}/zombie/zombie.png"),
                &format!("{ENTITY}/zombie/zombie_e.png"),
                &format!("{ENTITY}/zombie/zombie_glow.png"),
            ],
            Some("_glow"),
        );
        assert_eq!(
            e.overlay("entity/zombie/zombie"),
            Some("entity/zombie/zombie_glow")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn no_overlays_no_entries() {
        let (root, e) = pack("none", &[&format!("{ENTITY}/zombie/zombie.png")], None);
        assert!(e.is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
