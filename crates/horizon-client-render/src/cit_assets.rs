//! The ids Custom Item Textures' files take in Horizon's atlas and model library. A CIT rule
//! names its textures and models by resource path, mostly under `optifine/cit/` where vanilla
//! never looks: those take the reserved namespace [`NAMESPACE`], their path the resource path's
//! (`assets/minecraft/optifine/cit/swords/flame.png` is
//! `horizon_cit:minecraft/optifine/cit/swords/flame`). A file already under `textures/` (or
//! `models/`) keeps its vanilla id, so it is not packed twice.

use horizon_optifine::cit::{Kind, Rule};
use std::collections::BTreeSet;

/// The namespace of CIT files outside `textures/` and `models/`.
pub const NAMESPACE: &str = "horizon_cit";

/// `<namespace>:<path>`, if both keep to the identifier rules (lower-case letters, digits,
/// `_.-`, and `/` in the path). CIT Resewn refuses the other paths too.
fn identifier(namespace: &str, path: &str) -> Option<String> {
    if namespace.is_empty() || path.is_empty() {
        return None;
    }
    horizon_client_core::identifier::parse_identifier(&format!("{namespace}:{path}"))
}

/// The id of a resource path ending in `.<ext>`: `assets/<ns>/<folder>/<p>` is `<ns>:<p>`, any
/// other `assets/<rest>` is `horizon_cit:<rest>`.
fn id(resource: &str, folder: &str, ext: &str) -> Option<String> {
    let rest = resource
        .strip_prefix("assets/")?
        .strip_suffix(ext)?
        .strip_suffix('.')?;
    if let Some((namespace, path)) = rest.split_once('/')
        && let Some(path) = path.strip_prefix(folder).and_then(|p| p.strip_prefix('/'))
    {
        // A pack's own `assets/horizon_cit/` would name another file than its id maps back to.
        return (namespace != NAMESPACE)
            .then(|| identifier(namespace, path))
            .flatten();
    }
    identifier(NAMESPACE, rest)
}

/// The atlas sprite id of a `.png` resource path.
pub fn sprite_id(resource: &str) -> Option<String> {
    id(resource, "textures", "png")
}

/// The model id of a `.json` resource path.
pub fn model_id(resource: &str) -> Option<String> {
    id(resource, "models", "json")
}

/// The resource path of a `horizon_cit` id, with the extension `ext`; `None` for other ids,
/// and for one that would leave the resource root (a pack model may write any id).
pub fn resource(id: &str, ext: &str) -> Option<String> {
    let rest = id.strip_prefix(NAMESPACE)?.strip_prefix(':')?;
    horizon_optifine::paths::valid(rest).then(|| format!("assets/{rest}.{ext}"))
}

/// Sprite ids the item rules draw: `Kind::Item` rules' `texture` and `texture.<name>` values.
/// (Textures named by CIT JSON models come in a later PR.) A path no id can name is skipped with
/// a warning.
pub fn item_sprites(rules: &[Rule]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for rule in rules.iter().filter(|r| r.kind == Kind::Item) {
        let named = rule.textures.iter().map(|(_, path)| path);
        for path in rule.texture.iter().chain(named) {
            match sprite_id(path) {
                Some(id) => {
                    out.insert(id);
                }
                None => log::warn!(
                    "custom item textures: {} names {path}, which is not a valid texture id",
                    rule.path
                ),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas_sources::texture_path;
    use std::path::Path;

    #[test]
    fn cit_paths_take_the_reserved_namespace_and_come_back() {
        let png = "assets/minecraft/optifine/cit/swords/flame.png";
        let id = sprite_id(png).unwrap();
        assert_eq!(id, "horizon_cit:minecraft/optifine/cit/swords/flame");
        assert_eq!(resource(&id, "png").as_deref(), Some(png));
        let json = "assets/minecraft/optifine/cit/swords/flame.json";
        let id = model_id(json).unwrap();
        assert_eq!(id, "horizon_cit:minecraft/optifine/cit/swords/flame");
        assert_eq!(resource(&id, "json").as_deref(), Some(json));
    }

    #[test]
    fn files_under_textures_and_models_keep_their_vanilla_ids() {
        let id = sprite_id("assets/minecraft/textures/item/stick.png");
        assert_eq!(id.as_deref(), Some("minecraft:item/stick"));
        let id = model_id("assets/gems/models/item/ruby.json");
        assert_eq!(id.as_deref(), Some("gems:item/ruby"));
        // A model id is not a resource of this module's own.
        assert_eq!(resource("minecraft:item/stick", "png"), None);
        // `textures` as a folder name elsewhere is still a CIT path.
        let id = sprite_id("assets/minecraft/optifine/cit/textures/a.png");
        assert_eq!(
            id.as_deref(),
            Some("horizon_cit:minecraft/optifine/cit/textures/a")
        );
    }

    #[test]
    fn paths_no_identifier_can_name_are_refused() {
        for path in [
            "assets/minecraft/optifine/cit/Flame.png",
            "assets/minecraft/optifine/cit/fire sword.png",
            "assets/minecraft/textures/item/Stick.png",
            "assets/minecraft/optifine/cit/a.json",
            "optifine/cit/a.png",
        ] {
            assert_eq!(sprite_id(path), None, "{path}");
        }
        assert_eq!(model_id("assets/minecraft/optifine/cit/A.json"), None);
        assert_eq!(
            sprite_id("assets//textures/item/x.png"),
            None,
            "no namespace"
        );
    }

    #[test]
    fn the_reserved_namespace_names_only_files_inside_the_root() {
        // A pack's own `horizon_cit` namespace would map back to another file.
        assert_eq!(sprite_id("assets/horizon_cit/textures/item/x.png"), None);
        // A model may write any id: only ones inside the root name a file.
        for id in [
            "horizon_cit:../../etc/x",
            "horizon_cit:a/../../x",
            "horizon_cit:a\\..\\x",
        ] {
            assert_eq!(resource(id, "png"), None, "{id}");
            // Nor does the texture's file: refused, not read from elsewhere.
            let file = texture_path(Path::new("/r"), id, "textures");
            assert!(file.starts_with("/r/assets") && !file.to_string_lossy().contains(".."));
        }
        let escape = texture_path(Path::new("/r"), "minecraft:../../../x", "textures");
        assert!(!escape.to_string_lossy().contains(".."), "{escape:?}");
    }

    #[test]
    fn both_id_forms_find_their_file() {
        let root = Path::new("/r");
        assert_eq!(
            texture_path(root, "horizon_cit:minecraft/optifine/cit/a", "textures"),
            root.join("assets/minecraft/optifine/cit/a.png")
        );
        assert_eq!(
            texture_path(root, "minecraft:item/stick", "textures"),
            root.join("assets/minecraft/textures/item/stick.png")
        );
    }
}
