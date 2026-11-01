//! OptiFine custom entity models over Horizon's mob model layers: `optifine/cem/<name>.jem` (or
//! `cem/<name>/<name>.jem`, and Entity Model Features' `emf/cem/`, which wins) maps to one layer
//! (`zombie.jem` to `zombie#main`, `sheep_wool.jem` to `sheep#wool`), its parts to the layer's
//! vanilla parts, and its boxes join or replace theirs. A layer without its own model takes the
//! one it falls back to: a baby's adult (OptiFine's fallbacks, and Entity Model Features' for
//! horses too), and the few of [`FALLBACKS`].
//!
//! Built when the packs load; a mob's draw only swaps the layer's part tree. Without CEM files
//! nothing is built and every model stays vanilla's, and a file no layer uses isn't parsed.
//!
//! The seam with random entity textures (ETF): a `.jem`'s own `texture` replaces the layer's
//! vanilla texture name, and the texture selection then works on that name as on vanilla's.
//! It paints the layer the file is for, not the overlays drawn on that layer's model (a wolf's
//! collar, a spider's eyes): those keep their textures over the new geometry.

use super::{MobType, Part, Polygon};
use horizon_optifine::cem::model::{Bone, EntityModel, MAX_BYTES, parse_jem};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::Path,
    sync::Arc,
};

/// The CEM models of the enabled packs, by model layer (`zombie#main`).
#[derive(Default)]
pub struct CemModels {
    layers: HashMap<String, CemLayer>,
}

/// A model layer as a `.jem` reshapes it.
pub struct CemLayer {
    pub model: Arc<Part>,
    /// The `.jem`'s animations, run after the vanilla animation.
    pub animation: Option<super::cem_anim::Animation>,
    /// The atlas name of the `.jem`'s texture (`entity/zombie/zombie_green`), drawn instead of
    /// the layer's vanilla texture.
    pub texture: Option<String>,
}

impl CemModels {
    /// The `.jem` files under `root` applied to the layers of `types`.
    pub fn load(root: &Path, types: &HashMap<String, Arc<MobType>>) -> Self {
        let files = jem_files(root);
        if files.is_empty() {
            return Self::default();
        }
        Self::from_files(root, &files, &vanilla_layers(types))
    }

    /// The `.jem` files under `root` applied to the `vanilla` layers.
    #[cfg(test)]
    fn over(root: &Path, vanilla: &HashMap<&str, &Arc<Part>>) -> Self {
        Self::from_files(root, &jem_files(root), vanilla)
    }

    /// `files` ([`jem_files`]) applied to the `vanilla` layers.
    fn from_files(
        root: &Path,
        files: &BTreeMap<String, String>,
        vanilla: &HashMap<&str, &Arc<Part>>,
    ) -> Self {
        // The file each layer takes, and whether the file's texture goes with it. The first
        // claim wins: every own file in name order, then the fallbacks of the names without a
        // file, in the order of the files they fall back to. Two names of one layer
        // (`llama_decor` and `trader_llama_decor` are both `llama#decor`) give it the first's.
        let mut plan: HashMap<String, (&str, bool)> = HashMap::new();
        for name in files.keys() {
            let key = layer_key(name);
            if vanilla.contains_key(key.as_str()) {
                plan.entry(key).or_insert((name.as_str(), true));
            }
        }
        for name in files.keys() {
            for (target, texture) in fallback_names(name) {
                if files.contains_key(&target) {
                    continue;
                }
                let key = layer_key(&target);
                if vanilla.contains_key(key.as_str()) {
                    plan.entry(key).or_insert((name.as_str(), texture));
                }
            }
        }
        let used: HashSet<&str> = plan.values().map(|(name, _)| *name).collect();
        let mut read = |path: &str| read_small(&root.join(path));
        let mut models: HashMap<&str, EntityModel> = HashMap::new();
        for (name, path) in files {
            if !used.contains(name.as_str()) {
                continue;
            }
            let Some(text) = read(path.as_str()) else {
                log::warn!(
                    "Horizon: custom entity model {path}: unreadable or over {MAX_BYTES} bytes"
                );
                continue;
            };
            let jem = match parse_jem(path, &text, &mut read) {
                Ok(jem) => jem,
                Err(e) => {
                    log::warn!("Horizon: custom entity model {e}");
                    continue;
                }
            };
            for warning in &jem.warnings {
                log::warn!("Horizon: custom entity model {warning}");
            }
            if let Some(texture) = &jem.texture
                && atlas_name(texture).is_none()
            {
                log::warn!(
                    "Horizon: custom entity model {path}: its texture {texture} is outside \
                     assets/minecraft/textures/, so not in the atlas: the vanilla one draws"
                );
            }
            models.insert(name.as_str(), jem);
        }
        let mut layers = HashMap::new();
        for (key, (name, texture)) in plan {
            let (Some(jem), Some(part)) = (models.get(name), vanilla.get(key.as_str())) else {
                continue;
            };
            let mut layer = CemLayer::build(part, jem, name);
            if !texture {
                layer.texture = None;
            }
            layers.insert(key, layer);
        }
        Self { layers }
    }

    pub fn layer(&self, key: &str) -> Option<&CemLayer> {
        self.layers.get(key)
    }
}

/// Every model layer of `types` by its key.
fn vanilla_layers(types: &HashMap<String, Arc<MobType>>) -> HashMap<&str, &Arc<Part>> {
    types
        .values()
        .flat_map(|t| t.layers.iter().map(|(k, v)| (k.as_str(), v)))
        .collect()
}

/// A pack file's text, None when it's missing or larger than a model may read: its size is
/// checked before anything is read.
fn read_small(path: &Path) -> Option<String> {
    let len = fs::metadata(path).ok()?.len();
    if len > MAX_BYTES as u64 {
        return None;
    }
    fs::read_to_string(path).ok()
}

/// Every `.jem` of the CEM folders by model name, sorted. `emf/cem` wins over `optifine/cem`
/// whichever form either uses; within one folder the flat `<name>.jem` wins over
/// `<name>/<name>.jem`.
fn jem_files(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for base in ["assets/minecraft/optifine/cem", "assets/minecraft/emf/cem"] {
        let Ok(entries) = fs::read_dir(root.join(base)) else {
            continue;
        };
        let mut folder = BTreeMap::new();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(model) = name.strip_suffix(".jem") {
                // A directory named `x.jem` isn't a model.
                if e.path().is_file() {
                    folder.insert(model.to_owned(), format!("{base}/{name}"));
                }
            } else {
                let nested = format!("{base}/{name}/{name}.jem");
                if root.join(&nested).is_file() {
                    folder.entry(name).or_insert(nested);
                }
            }
        }
        out.extend(folder);
    }
    out
}

/// The CEM model names with a layer key of their own; any other name `n` is `n#main`.
const LAYERS: [(&str, &str); 23] = [
    ("sheep_wool", "sheep#wool"),
    ("creeper_charge", "creeper#armor"),
    ("slime_outer", "slime#outer"),
    ("drowned_outer", "drowned#outer"),
    ("stray_outer", "stray#outer"),
    ("bogged_outer", "bogged#outer"),
    ("wither_armor", "wither#armor"),
    ("breeze_wind", "breeze#wind"),
    // Horizon draws both llamas' decor on one layer: the first file by name, `llama_decor`,
    // shapes it when a pack has both.
    ("llama_decor", "llama#decor"),
    ("trader_llama_decor", "llama#decor"),
    ("cat_collar", "cat#collar"),
    ("pig_saddle", "pig#saddle"),
    ("strider_saddle", "strider#saddle"),
    ("nautilus_saddle", "nautilus#saddle"),
    ("tropical_fish_a", "tropical_fish_small#main"),
    ("tropical_fish_b", "tropical_fish_large#main"),
    ("tropical_fish_pattern_a", "tropical_fish_small#pattern"),
    ("tropical_fish_pattern_b", "tropical_fish_large#pattern"),
    ("puffer_fish_big", "pufferfish_big#main"),
    ("puffer_fish_medium", "pufferfish_medium#main"),
    ("puffer_fish_small", "pufferfish_small#main"),
    // One layer for both too: `skeleton_horse_armor` wins over `zombie_horse_armor`.
    ("skeleton_horse_armor", "undead_horse_armor#main"),
    ("zombie_horse_armor", "undead_horse_armor#main"),
];

/// The model layer a CEM model name reshapes.
fn layer_key(name: &str) -> String {
    // `<model>_baby[_<layer>]`: the adult's layer on the baby model.
    if let Some((base, rest)) = name.split_once("_baby") {
        let adult = layer_key(&format!("{base}{rest}"));
        let (model, layer) = adult.split_once('#').unwrap_or((&adult, "main"));
        return format!("{model}_baby#{layer}");
    }
    match LAYERS.iter().find(|(n, _)| *n == name) {
        Some((_, key)) => (*key).to_owned(),
        None => format!("{name}#main"),
    }
}

/// Models without a file of their own that take another's (OptiFine's and Entity Model
/// Features' tables), besides the babies: (model, the model it takes).
const FALLBACKS: [(&str, &str); 4] = [
    ("cat_collar", "cat"),
    ("player_slim", "player"),
    ("salmon_large", "salmon"),
    ("salmon_small", "salmon"),
];

/// The model names that take `name`'s file when they have none, each with whether its texture
/// goes along: the babies of an adult and [`FALLBACKS`]. An overlay of another layer (the cat's
/// collar from `cat`) takes only the geometry, its own texture stays.
fn fallback_names(name: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = FALLBACKS
        .iter()
        .filter(|(_, from)| *from == name)
        .map(|(to, _)| {
            let same_layer = layer_key(to).split_once('#').map(|(_, l)| l)
                == layer_key(name).split_once('#').map(|(_, l)| l);
            ((*to).to_owned(), same_layer)
        })
        .collect();
    if !name.contains("_baby") {
        // The baby nautilus has no fallback (OptiFine's and Entity Model Features' tables).
        out.extend(
            baby_names(name)
                .filter(|baby| !baby.starts_with("nautilus_baby"))
                .map(|baby| (baby, true)),
        );
    }
    out
}

/// The baby model names that fall back to the adult `name`: `_baby` before any of its
/// `_` words or at its end (`sheep_wool` gives `sheep_baby_wool`).
fn baby_names(name: &str) -> impl Iterator<Item = String> + '_ {
    let cuts = name.match_indices('_').map(|(i, _)| i).chain([name.len()]);
    cuts.map(move |i| format!("{}_baby{}", &name[..i], &name[i..]))
}

/// The vanilla part an OptiFine part name of model `model` means. Names OptiFine kept from
/// the game's own code map to themselves.
fn vanilla_part(model: &str, part: &str) -> String {
    // OptiFine numbers from 1, the game from 0.
    let n = |prefix: &str| -> Option<usize> {
        part.strip_prefix(prefix)?.parse().ok().filter(|i| *i >= 1)
    };
    let quadruped = matches!(
        model,
        "cow"
            | "cold_cow"
            | "warm_cow"
            | "mooshroom"
            | "pig"
            | "cold_pig"
            | "warm_pig"
            | "pig_saddle"
            | "sheep"
            | "sheep_wool"
            | "polar_bear"
            | "panda"
            | "turtle"
            | "llama"
            | "llama_decor"
            | "trader_llama"
            | "trader_llama_decor"
            | "goat"
            | "fox"
            | "wolf"
            | "wolf_armor"
            | "wolf_collar"
            | "ravager"
            | "creeper"
            | "creeper_charge"
            | "axolotl"
    );
    const LEGS: [&str; 4] = [
        "right_hind_leg",
        "left_hind_leg",
        "right_front_leg",
        "left_front_leg",
    ];
    const SPIDER_LEGS: [&str; 8] = [
        "right_hind_leg",
        "left_hind_leg",
        "right_middle_hind_leg",
        "left_middle_hind_leg",
        "right_middle_front_leg",
        "left_middle_front_leg",
        "right_front_leg",
        "left_front_leg",
    ];
    let spider = matches!(model, "spider" | "cave_spider");
    let renamed: Option<String> = match (model, part) {
        (_, "headwear") => Some("hat".into()),
        (_, "back_left_leg") => Some("left_hind_leg".into()),
        (_, "back_right_leg") => Some("right_hind_leg".into()),
        (_, "front_left_leg") => Some("left_front_leg".into()),
        (_, "front_right_leg") => Some("right_front_leg".into()),
        (_, _) if quadruped && n("leg").is_some_and(|i| (1..=4).contains(&i)) => {
            n("leg").map(|i| LEGS[i - 1].into())
        }
        (_, _) if spider && n("leg").is_some_and(|i| (1..=8).contains(&i)) => {
            n("leg").map(|i| SPIDER_LEGS[i - 1].into())
        }
        (_, "neck") if spider => Some("body0".into()),
        (_, "body") if spider => Some("body1".into()),
        ("villager" | "wandering_trader" | "witch", "headwear2") => Some("hat_rim".into()),
        ("villager" | "wandering_trader" | "witch", "bodywear") => Some("jacket".into()),
        ("chicken" | "cold_chicken" | "warm_chicken", "bill") => Some("beak".into()),
        ("chicken" | "cold_chicken" | "warm_chicken", "chin") => Some("red_thing".into()),
        ("llama" | "llama_decor" | "trader_llama" | "trader_llama_decor", "chest_right") => {
            Some("right_chest".into())
        }
        ("llama" | "llama_decor" | "trader_llama" | "trader_llama_decor", "chest_left") => {
            Some("left_chest".into())
        }
        ("turtle", "body2") => Some("egg_belly".into()),
        ("ravager", "jaw") => Some("mouth".into()),
        ("bee" | "warden", "body") => Some("bone".into()),
        ("bee" | "warden", "torso") => Some("body".into()),
        ("snow_golem", "body") => Some("upper_body".into()),
        ("snow_golem", "body_bottom") => Some("lower_body".into()),
        ("snow_golem", "right_hand") => Some("right_arm".into()),
        ("snow_golem", "left_hand") => Some("left_arm".into()),
        ("slime" | "slime_outer", "body") => Some("cube".into()),
        ("magma_cube", "core") => Some("inside_cube".into()),
        ("magma_cube", _) => n("segment").map(|i| format!("cube{}", i - 1)),
        ("blaze", _) => n("stick").map(|i| format!("part{}", i - 1)),
        ("ghast" | "happy_ghast" | "happy_ghast_ropes" | "squid" | "glow_squid", _) => {
            n("tentacle").map(|i| format!("tentacle{}", i - 1))
        }
        ("silverfish", _) => n("body")
            .map(|i| format!("segment{}", i - 1))
            .or_else(|| n("wing").map(|i| format!("layer{}", i - 1))),
        ("endermite", _) => n("body").map(|i| format!("segment{}", i - 1)),
        ("guardian" | "elder_guardian", _) => n("spine")
            .map(|i| format!("spike{}", i - 1))
            .or_else(|| n("tail").map(|i| format!("tail{}", i - 1))),
        ("wither" | "wither_armor", _) => n("body")
            .and_then(|i| ["shoulders", "ribcage", "tail"].get(i - 1).copied())
            .or_else(|| {
                n("head").and_then(|i| {
                    ["center_head", "right_head", "left_head"]
                        .get(i - 1)
                        .copied()
                })
            })
            .map(str::to_owned),
        ("phantom", "left_wing") => Some("left_wing_base".into()),
        ("phantom", "right_wing") => Some("right_wing_base".into()),
        ("phantom", "tail") => Some("tail_base".into()),
        ("phantom", "tail2") => Some("tail_tip".into()),
        ("cat" | "ocelot" | "cat_collar", "tail") => Some("tail1".into()),
        ("bat", "outer_right_wing") => Some("right_wing_tip".into()),
        ("bat", "outer_left_wing") => Some("left_wing_tip".into()),
        ("cod" | "tropical_fish_a" | "tropical_fish_b", "fin_back" | "fin_top") => {
            Some("top_fin".into())
        }
        // The big and medium pufferfish have blue fins; the small one has plain `right_fin`s
        // (the fin animation in aquatic.rs).
        ("puffer_fish_big" | "puffer_fish_medium", "fin_right") => Some("right_blue_fin".into()),
        ("puffer_fish_big" | "puffer_fish_medium", "fin_left") => Some("left_blue_fin".into()),
        // RabbitModel's hind feet (rabbit.rs).
        ("rabbit", "left_foot") => Some("left_hind_foot".into()),
        ("rabbit", "right_foot") => Some("right_hind_foot".into()),
        (_, "fin_right") => Some("right_fin".into()),
        (_, "fin_left") => Some("left_fin".into()),
        (_, "fin_bottom") => Some("bottom_fin".into()),
        ("cod", "tail") => Some("tail_fin".into()),
        ("sniffer", _) => part
            .strip_prefix("middle_")
            .and_then(|side| side.strip_suffix("_leg"))
            .map(|side| format!("{side}_mid_leg")),
        // `spikes_<front>_<top>` and `hair_<right>_<bottom>`.
        (_, _) if part.starts_with("spikes_") => part["spikes_".len()..]
            .split_once('_')
            .map(|(along, side)| format!("{side}_{along}_fin")),
        ("strider" | "strider_saddle", _) => part
            .strip_prefix("hair_")
            .map(|rest| format!("{rest}_bristle")),
        ("horse" | "donkey" | "mule" | "skeleton_horse" | "zombie_horse", "neck") => {
            Some("head_parts".into())
        }
        ("horse" | "donkey" | "mule" | "skeleton_horse" | "zombie_horse", "mouth") => {
            Some("upper_mouth".into())
        }
        _ => None,
    };
    renamed.unwrap_or_else(|| part.to_owned())
}

impl CemLayer {
    /// `vanilla` reshaped by `jem`, the model `name`'s file.
    fn build(vanilla: &Part, jem: &EntityModel, name: &str) -> Self {
        let model = name.replace("_baby", "");
        let mut out = vanilla.clone();
        let mut count = 0;
        let mut ids = HashMap::new();
        // Each entry's `this` and `part`, and its animations.
        let mut entries = Vec::new();
        for p in &jem.parts {
            let target = vanilla_part(&model, &p.part);
            let target = if super::render::find_part(&out, &target).is_some() {
                target
            } else {
                p.part.clone()
            };
            let mut bone = part(&p.bone, &mut count, &mut ids);
            bone.pose[6..9].fill(p.scale);
            let Some(node) = find_mut(&mut out, &target) else {
                log::warn!("Horizon: custom entity model {name}: no part {}", p.part);
                continue;
            };
            if !p.attach {
                node.polygons.clear();
            }
            // A replacing entry stands in for the vanilla part; an attached one is its own.
            let this = if p.attach {
                bone.name.clone()
            } else {
                target.clone()
            };
            entries.push((this, target, p.animations.as_slice()));
            node.children.push(bone);
        }
        let names = super::cem_anim::Names {
            model: &out,
            vanilla: &|p| vanilla_part(&model, p),
            ids: &ids,
        };
        let animation = super::cem_anim::compile_animation(name, &names, &entries);
        Self {
            model: Arc::new(out),
            animation,
            texture: jem.texture.as_deref().and_then(atlas_name),
        }
    }
}

/// The atlas name of a texture resource path: `assets/minecraft/textures/entity/x.png` is
/// `entity/x`. Textures outside a `textures/` folder are not in the atlas.
fn atlas_name(path: &str) -> Option<String> {
    let rest = path.strip_prefix("assets/minecraft/textures/")?;
    Some(rest.strip_suffix(".png").unwrap_or(rest).to_owned())
}

/// The first part named `name`, depth first (`ModelPart.getChild` through the tree).
fn find_mut<'a>(part: &'a mut Part, name: &str) -> Option<&'a mut Part> {
    if part.name == name {
        return Some(part);
    }
    part.children.iter_mut().find_map(|c| find_mut(c, name))
}

/// A baked bone as a model part. Its name, `cem#<n>`, is unique in the model, so its pose is
/// its own whatever the files call their ids.
/// `ids` gets the bone's id, if it has one, under that name.
fn part(bone: &Bone, count: &mut usize, ids: &mut HashMap<String, String>) -> Part {
    let name = format!("cem#{count}");
    *count += 1;
    if let Some(id) = &bone.id {
        ids.insert(name.clone(), id.clone());
    }
    let [x, y, z] = bone.pivot;
    let [rx, ry, rz] = bone.rotation;
    Part {
        name,
        pose: [x, y, z, rx, ry, rz, 1.0, 1.0, 1.0],
        polygons: bone
            .quads
            .iter()
            .map(|q| Polygon {
                normal: q.normal,
                vertices: q.vertices,
            })
            .collect(),
        children: bone.children.iter().map(|c| part(c, count, ids)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(name: &str, children: Vec<Part>) -> Part {
        Part {
            name: name.into(),
            pose: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            polygons: vec![Polygon {
                normal: [0.0, 0.0, -1.0],
                vertices: [[0.0; 5]; 4],
            }],
            children,
        }
    }
    fn get<'a>(part: &'a Part, name: &str) -> &'a Part {
        fn walk<'a>(p: &'a Part, name: &str) -> Option<&'a Part> {
            (p.name == name)
                .then_some(p)
                .or_else(|| p.children.iter().find_map(|c| walk(c, name)))
        }
        walk(part, name).unwrap()
    }

    #[test]
    fn layer_names() {
        for (name, key) in [
            ("zombie", "zombie#main"),
            ("zombie_baby", "zombie_baby#main"),
            ("sheep_wool", "sheep#wool"),
            ("sheep_baby_wool", "sheep_baby#wool"),
            ("drowned_baby_outer", "drowned_baby#outer"),
            ("cold_chicken", "cold_chicken#main"),
            ("puffer_fish_big", "pufferfish_big#main"),
            ("tropical_fish_pattern_b", "tropical_fish_large#pattern"),
        ] {
            assert_eq!(layer_key(name), key, "{name}");
        }
        assert_eq!(
            baby_names("sheep_wool").collect::<Vec<_>>(),
            ["sheep_baby_wool", "sheep_wool_baby"]
        );
    }

    #[test]
    fn part_names() {
        for (model, part, vanilla) in [
            ("zombie", "headwear", "hat"),
            ("zombie", "left_arm", "left_arm"),
            ("cow", "leg1", "right_hind_leg"),
            ("cow", "leg4", "left_front_leg"),
            ("spider", "leg5", "right_middle_front_leg"),
            ("spider", "neck", "body0"),
            ("ghast", "tentacle1", "tentacle0"),
            ("blaze", "stick12", "part11"),
            ("camel", "back_left_leg", "left_hind_leg"),
            ("villager", "headwear2", "hat_rim"),
            ("wither", "head2", "right_head"),
            ("strider", "hair_left_top", "left_top_bristle"),
            ("puffer_fish_big", "spikes_front_top", "top_front_fin"),
            ("squid", "tentacle0", "tentacle0"),
            ("puffer_fish_medium", "fin_left", "left_blue_fin"),
            ("puffer_fish_small", "fin_right", "right_fin"),
            ("rabbit", "left_foot", "left_hind_foot"),
        ] {
            assert_eq!(vanilla_part(model, part), vanilla, "{model} {part}");
        }
    }

    #[test]
    fn files_reshape_their_layers_and_babies_fall_back() {
        let root = std::env::temp_dir().join(format!("horizon-cem-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cem = root.join("assets/minecraft/optifine/cem");
        fs::create_dir_all(cem.join("zombie")).unwrap();
        let box_ = r#""boxes": [{"coordinates": [0, 0, 0, 1, 1, 1], "textureOffset": [0, 0]}]"#;
        fs::write(
            cem.join("zombie/zombie.jem"),
            format!(
                r#"{{"texture": "textures/entity/zombie/green.png", "models": [
                    {{"part": "head", {box_}}},
                    {{"part": "headwear", "attach": true, "scale": 2, {box_}}},
                    {{"part": "tail", {box_}}}]}}"#
            ),
        )
        .unwrap();
        // Picked over optifine/cem's, though nothing reads EMF's here but its folder.
        fs::create_dir_all(root.join("assets/minecraft/emf/cem")).unwrap();
        fs::write(
            root.join("assets/minecraft/emf/cem/cow.jem"),
            format!(r#"{{"models": [{{"part": "leg1", {box_}}}]}}"#),
        )
        .unwrap();
        fs::write(cem.join("cow.jem"), "{").unwrap();
        let zombie = Arc::new(plane(
            "root",
            vec![plane("head", vec![plane("hat", vec![])])],
        ));
        let cow = Arc::new(plane("root", vec![plane("right_hind_leg", vec![])]));
        let vanilla: HashMap<&str, &Arc<Part>> = [
            ("zombie#main", &zombie),
            ("zombie_baby#main", &zombie),
            ("cow#main", &cow),
        ]
        .into_iter()
        .collect();
        let models = CemModels::over(&root, &vanilla);
        fs::remove_dir_all(&root).unwrap();

        let adult = models.layer("zombie#main").unwrap();
        assert_eq!(adult.texture.as_deref(), Some("entity/zombie/green"));
        let head = get(&adult.model, "head");
        // Replaced: the head's own face goes, its hat stays, the custom box joins as a child.
        assert_eq!(head.polygons.len(), 0);
        assert_eq!(get(&adult.model, "cem#0").polygons.len(), 6);
        // Attached: the hat keeps its face beside the scaled custom box.
        let hat = get(&adult.model, "hat");
        assert_eq!(hat.polygons.len(), 1);
        assert_eq!(get(hat, "cem#1").pose[6..], [2.0, 2.0, 2.0]);
        // The baby has no file of its own: it takes the adult's.
        let baby = models.layer("zombie_baby#main").unwrap();
        assert_eq!(get(&baby.model, "head").polygons.len(), 0);
        let leg = get(&models.layer("cow#main").unwrap().model, "right_hind_leg");
        assert_eq!((leg.polygons.len(), leg.children.len()), (0, 1));
        assert!(models.layer("sheep#main").is_none());
    }

    const BOX: &str = r#""boxes": [{"coordinates": [0, 0, 0, 1, 1, 1], "textureOffset": [0, 0]}]"#;

    /// A `.jem` with one entry on `part`, attached or replacing it.
    fn jem(part: &str, attach: bool) -> String {
        format!(r#"{{"models": [{{"part": "{part}", "attach": {attach}, {BOX}}}]}}"#)
    }

    /// A fresh pack folder for the test `test` holding `files` (paths under `assets/minecraft/`).
    fn pack(test: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("horizon-cem-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, text) in files {
            let path = root.join("assets/minecraft").join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        root
    }

    /// The models of the pack `files` over the `layers`, the same tree for each.
    fn models(test: &str, files: &[(&str, &str)], layers: &[&str], tree: &Part) -> CemModels {
        let root = pack(test, files);
        let tree = Arc::new(tree.clone());
        let vanilla: HashMap<&str, &Arc<Part>> = layers.iter().map(|k| (*k, &tree)).collect();
        let models = CemModels::over(&root, &vanilla);
        fs::remove_dir_all(&root).unwrap();
        models
    }

    /// Whether the layer `key`'s part `name` was replaced (lost its own face) or had a box
    /// attached (kept it); None without the layer. Either way the box joins as a child.
    fn attached(models: &CemModels, key: &str, name: &str) -> Option<bool> {
        let part = get(&models.layer(key)?.model, name);
        assert_eq!(part.children.len(), 1, "{key} {name}: one custom box");
        Some(part.polygons.len() == 1)
    }

    #[test]
    fn emf_files_win_and_a_flat_file_beats_its_folder() {
        let (replace, attach) = (jem("leg1", false), jem("leg1", true));
        let files = [
            // EMF's folder form over OptiFine's flat file.
            ("optifine/cem/cow.jem", attach.as_str()),
            ("emf/cem/cow/cow.jem", replace.as_str()),
            // One folder: the flat file over the folder form.
            ("optifine/cem/pig.jem", replace.as_str()),
            ("optifine/cem/pig/pig.jem", attach.as_str()),
            // A directory named `sheep.jem` is no file: the folder form stands.
            ("optifine/cem/sheep.jem/readme.txt", ""),
            ("optifine/cem/sheep/sheep.jem", attach.as_str()),
        ];
        let tree = plane("root", vec![plane("right_hind_leg", vec![])]);
        let models = models(
            "precedence",
            &files,
            &["cow#main", "pig#main", "sheep#main"],
            &tree,
        );
        assert_eq!(attached(&models, "cow#main", "right_hind_leg"), Some(false));
        assert_eq!(attached(&models, "pig#main", "right_hind_leg"), Some(false));
        assert_eq!(
            attached(&models, "sheep#main", "right_hind_leg"),
            Some(true)
        );
    }

    #[test]
    fn a_baby_with_its_own_file_takes_it_not_the_adults() {
        let (adult, baby) = (jem("head", false), jem("head", true));
        let files = [
            ("optifine/cem/zombie.jem", adult.as_str()),
            ("optifine/cem/zombie_baby.jem", baby.as_str()),
        ];
        let tree = plane("root", vec![plane("head", vec![])]);
        let models = models("baby", &files, &["zombie#main", "zombie_baby#main"], &tree);
        assert_eq!(attached(&models, "zombie#main", "head"), Some(false));
        assert_eq!(attached(&models, "zombie_baby#main", "head"), Some(true));
    }

    #[test]
    fn two_names_of_one_layer_give_it_the_first_by_name() {
        let (llama, trader) = (jem("body", false), jem("body", true));
        let files = [
            ("optifine/cem/trader_llama_decor.jem", trader.as_str()),
            ("optifine/cem/llama_decor.jem", llama.as_str()),
        ];
        let tree = plane("root", vec![plane("body", vec![])]);
        // Written trader first, `llama_decor` still shapes the layer, and the baby's (both
        // fall back to the adult files).
        let models = models("decor", &files, &["llama#decor", "llama_baby#decor"], &tree);
        assert_eq!(attached(&models, "llama#decor", "body"), Some(false));
        assert_eq!(attached(&models, "llama_baby#decor", "body"), Some(false));
    }

    #[test]
    fn other_models_fall_back_but_the_baby_nautilus_does_not() {
        let (replace, attach) = (jem("body", false), jem("body", true));
        let cat = format!(
            r#"{{"texture": "textures/entity/cat/custom.png", "models": [
                {{"part": "body", {BOX}}}]}}"#
        );
        let files = [
            ("optifine/cem/salmon.jem", replace.as_str()),
            ("optifine/cem/salmon_large.jem", attach.as_str()),
            ("optifine/cem/nautilus.jem", replace.as_str()),
            ("optifine/cem/cat.jem", cat.as_str()),
        ];
        let tree = plane("root", vec![plane("body", vec![])]);
        let models = models(
            "fallbacks",
            &files,
            &[
                "salmon#main",
                "salmon_small#main",
                "salmon_large#main",
                "nautilus#main",
                "nautilus_baby#main",
                "cat#main",
                "cat#collar",
            ],
            &tree,
        );
        // The small salmon has no file: salmon.jem's. The large one has its own.
        assert_eq!(attached(&models, "salmon_small#main", "body"), Some(false));
        assert_eq!(attached(&models, "salmon_large#main", "body"), Some(true));
        assert_eq!(attached(&models, "nautilus#main", "body"), Some(false));
        assert!(models.layer("nautilus_baby#main").is_none());
        // The collar takes the cat's geometry, but its own collar texture stays.
        let texture = |key: &str| models.layer(key).unwrap().texture.clone();
        assert_eq!(texture("cat#main").as_deref(), Some("entity/cat/custom"));
        assert_eq!(attached(&models, "cat#collar", "body"), Some(false));
        assert_eq!(texture("cat#collar"), None);
    }

    #[test]
    fn an_unknown_part_is_skipped_and_the_rest_applies() {
        let file =
            format!(r#"{{"models": [{{"part": "wings", {BOX}}}, {{"part": "head", {BOX}}}]}}"#);
        let tree = plane("root", vec![plane("head", vec![])]);
        let models = models(
            "unknown",
            &[("optifine/cem/zombie.jem", file.as_str())],
            &["zombie#main"],
            &tree,
        );
        assert_eq!(attached(&models, "zombie#main", "head"), Some(false));
        fn custom(p: &Part) -> usize {
            usize::from(p.name.starts_with("cem#")) + p.children.iter().map(custom).sum::<usize>()
        }
        assert_eq!(custom(&models.layer("zombie#main").unwrap().model), 1);
    }

    /// A spider whose `.jem` replaces its body: the body draws with the `.jem`'s texture, the
    /// eyes (an overlay on the body's model) with theirs on the new geometry, and a texture the
    /// atlas lacks leaves the vanilla one.
    #[test]
    fn a_jem_texture_paints_the_body_and_leaves_its_overlays() {
        use super::super::{Anim, Mob, MobData, Species, render::render};
        let mut kind = (*super::super::sensor_scan_tests::kind()).clone();
        let body = Part {
            name: "root".into(),
            pose: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            polygons: Vec::new(),
            children: vec![plane("body1", vec![])],
        };
        kind.id = "minecraft:spider".into();
        kind.species = Species::Spider;
        kind.anim = Anim::Spider;
        kind.texture = "entity/spider/spider".into();
        kind.model = Arc::new(body.clone());
        kind.layers = HashMap::from([("spider#main".to_owned(), Arc::new(body.clone()))]);
        let kind = Arc::new(kind);
        let file = format!(
            r#"{{"texture": "textures/entity/spider/custom.png", "models": [
                {{"part": "body", {BOX}}}]}}"#
        );
        let models = models(
            "render",
            &[("optifine/cem/spider.jem", file.as_str())],
            &["spider#main"],
            &body,
        );
        let data = MobData {
            states: Vec::new(),
            placements: Default::default(),
            heightmaps: Default::default(),
            animations: Default::default(),
            biome_tags: Default::default(),
            entity_tags: Default::default(),
            villager_hats: Default::default(),
            cem: std::sync::RwLock::new(Arc::new(models)),
            random_textures: Default::default(),
            pois: Default::default(),
            species_placements: Default::default(),
        };
        let mob = Mob::new(1, kind, horizon_core::geometry::Vec3::ZERO, 0.0);
        // Three sprites far apart: which one a vertex's UV falls in says what it drew with.
        let rect = |name: &str| -> Option<[f32; 4]> {
            match name {
                "entity/spider/custom" => Some([0.0, 0.0, 0.2, 0.2]),
                "entity/spider/spider_eyes" => Some([0.4, 0.4, 0.6, 0.6]),
                "entity/spider/spider" => Some([0.8, 0.8, 1.0, 1.0]),
                _ => None,
            }
        };
        let sprites = |has_custom: bool| {
            let atlas = |name: &str| rect(name).filter(|_| has_custom || !name.ends_with("custom"));
            let mut vertices = Vec::new();
            render(
                &mob,
                0.0,
                glam::DVec3::ZERO,
                &atlas,
                0xf0f0,
                &data,
                &mut vertices,
            );
            let mut counts = [0; 3];
            for v in &vertices {
                let i = match v.uv[0] {
                    u if u <= 0.2 => 0,
                    u if (0.4..=0.6).contains(&u) => 1,
                    u if u >= 0.8 => 2,
                    u => panic!("a UV between the sprites: {u}"),
                };
                counts[i] += 1;
            }
            counts
        };
        // The custom box's six faces, four corners each, for the body and again for the eyes;
        // the replaced body part's own face is gone.
        assert_eq!(sprites(true), [24, 24, 0]);
        assert_eq!(sprites(false), [0, 24, 24]);
    }

    /// Every layer a model name maps to, and every part a part name is renamed to, is in
    /// Horizon's own models.
    #[test]
    #[ignore = "requires prepared entity models in HORIZON_TEST_RESOURCES"]
    fn names_reach_horizons_models() {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES").expect("HORIZON_TEST_RESOURCES");
        let types: HashMap<String, Arc<MobType>> = super::super::MobTypes::load(Path::new(&root))
            .unwrap()
            .types
            .into_iter()
            .map(|(k, v)| (k, Arc::new(v)))
            .collect();
        let vanilla = vanilla_layers(&types);
        let mut missing = Vec::new();
        for (name, key) in LAYERS {
            if !vanilla.contains_key(key) {
                missing.push(format!("{name}: no layer {key}"));
            }
        }
        for (name, _) in FALLBACKS {
            let key = layer_key(name);
            if !vanilla.contains_key(key.as_str()) {
                missing.push(format!("{name}: no layer {key}"));
            }
        }
        for (model, part) in [
            ("zombie", "headwear"),
            ("cow", "leg1"),
            ("cow", "leg4"),
            ("camel", "back_left_leg"),
            ("camel", "front_right_leg"),
            ("spider", "leg1"),
            ("spider", "leg8"),
            ("spider", "neck"),
            ("spider", "body"),
            ("villager", "headwear2"),
            ("villager", "bodywear"),
            ("witch", "headwear2"),
            ("witch", "bodywear"),
            ("chicken", "bill"),
            ("chicken", "chin"),
            ("llama", "chest_right"),
            ("llama", "chest_left"),
            ("turtle", "body2"),
            ("ravager", "jaw"),
            ("bee", "body"),
            ("bee", "torso"),
            ("warden", "body"),
            ("warden", "torso"),
            ("snow_golem", "body"),
            ("snow_golem", "body_bottom"),
            ("snow_golem", "right_hand"),
            ("snow_golem", "left_hand"),
            ("slime", "body"),
            ("magma_cube", "core"),
            ("magma_cube", "segment1"),
            ("blaze", "stick1"),
            ("ghast", "tentacle1"),
            ("squid", "tentacle1"),
            ("silverfish", "body1"),
            ("silverfish", "wing1"),
            ("endermite", "body1"),
            ("guardian", "spine1"),
            ("guardian", "tail1"),
            ("wither", "body1"),
            ("wither", "head1"),
            ("phantom", "left_wing"),
            ("phantom", "tail2"),
            ("cat", "tail"),
            ("ocelot", "tail"),
            ("bat", "outer_right_wing"),
            ("cod", "fin_back"),
            ("cod", "fin_right"),
            ("cod", "tail"),
            ("tropical_fish_b", "fin_top"),
            ("tropical_fish_b", "fin_bottom"),
            ("puffer_fish_big", "fin_right"),
            ("puffer_fish_medium", "fin_left"),
            ("puffer_fish_small", "fin_right"),
            ("puffer_fish_big", "spikes_front_top"),
            ("rabbit", "left_foot"),
            ("rabbit", "right_foot"),
            ("sniffer", "middle_left_leg"),
            ("strider", "hair_left_top"),
            ("horse", "neck"),
            ("horse", "mouth"),
        ] {
            let key = layer_key(model);
            let target = vanilla_part(model, part);
            let found = vanilla
                .get(key.as_str())
                .is_some_and(|tree| super::super::render::find_part(tree, &target).is_some());
            if !found {
                missing.push(format!("{model} {part}: no part {target} in {key}"));
            }
        }
        assert!(missing.is_empty(), "{missing:#?}");
    }
}
