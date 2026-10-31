//! API mods (docs/native-mods.md, "API mods"): native mods without mixins. horizon-mixins never sees
//! them. Their crate is staged as `native-crates/<mod>/<crate>/`, like a mixin mod's, and a small
//! generated package, `native-game/`, is the game's executable: it links the horizon-client library
//! and the mods, so only it depends on them. Changing an API mod recompiles that mod and this
//! package and links the game; horizon-client stays compiled.
use super::{crates, Bundle, MANIFEST};
use horizon_mods::index::native::api_crate;
use std::fs;
use std::path::Path;

/// The generated executable's folder in the staged workspace, and its package.
pub(super) const GAME: &str = "native-game";
pub(super) const PACKAGE: &str = "horizon-game";

/// Stages the API mods `mods` into the workspace `stage`: their crates, the `native-game` package
/// and both as workspace members. Nothing is written for none.
pub(super) fn stage(stage: &Path, mods: &[&Bundle]) -> Result<(), String> {
    if mods.is_empty() {
        return Ok(());
    }
    let mut members = Vec::new();
    let mut entries = Vec::new();
    let mut deps = toml::Table::new();
    for (name, path) in [
        ("horizon-client", "../crates/horizon-client"),
        ("horizon-threads", "../crates/horizon-threads"),
        ("horizon-mods", "../crates/horizon-mods"),
        ("horizon-mod-api", "../crates/horizon-mod-api"),
    ] {
        deps.insert(name.into(), path_dep(path, None));
    }
    for bundle in mods {
        let api = bundle.api.as_ref().ok_or("not an API mod")?;
        let source = bundle.dir.join(MANIFEST);
        let text = fs::read_to_string(&source).map_err(|e| format!("{}: {e}", source.display()))?;
        let mixin: toml::Value =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", source.display()))?;
        let from = bundle.dir.join(api_crate(&bundle.id, api, &mixin)?);
        let relative = format!("{}/{}/{}", crates::DIR, bundle.id, api.krate);
        let to = stage.join(&relative);
        let _ = fs::remove_dir_all(&to);
        crate::backend::copy_dir(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
        let package = point_api_at(&to.join("Cargo.toml"), "../../../crates/horizon-mod-api")?;
        let alias = format!("mod_{}", bundle.id);
        deps.insert(
            alias.clone(),
            path_dep(&format!("../{relative}"), Some(&package)),
        );
        entries.push(format!(
            "        ({:?}, {alias}::{}()),",
            bundle.id, api.entry
        ));
        members.push(relative);
    }
    let game = stage.join(GAME);
    fs::create_dir_all(game.join("src")).map_err(|e| e.to_string())?;
    let manifest = toml::Table::from_iter([
        (
            "package".into(),
            toml::Value::Table(toml::Table::from_iter([
                ("name".into(), PACKAGE.into()),
                ("version".into(), "0.1.0".into()),
                ("edition".into(), "2024".into()),
                ("publish".into(), false.into()),
            ])),
        ),
        (
            "bin".into(),
            toml::Value::Array(vec![toml::Value::Table(toml::Table::from_iter([
                ("name".into(), "horizon-client".into()),
                ("path".into(), "src/main.rs".into()),
            ]))]),
        ),
        ("dependencies".into(), toml::Value::Table(deps)),
    ]);
    write(
        &game.join("Cargo.toml"),
        &toml::to_string(&manifest).map_err(|e| e.to_string())?,
    )?;
    write(&game.join("src/main.rs"), &main_rs(&entries))?;
    members.push(GAME.into());
    let root = stage.join("Cargo.toml");
    let text = fs::read_to_string(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let mut workspace: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let list = workspace
        .get_mut("workspace")
        .and_then(|w| w.get_mut("members"))
        .and_then(|m| m.as_array_mut())
        .ok_or("the workspace's Cargo.toml has no members")?;
    for member in members {
        if !list.iter().any(|m| m.as_str() == Some(&member)) {
            list.push(member.into());
        }
    }
    write(
        &root,
        &toml::to_string(&workspace).map_err(|e| e.to_string())?,
    )
}

fn path_dep(path: &str, package: Option<&str>) -> toml::Value {
    let mut dep = toml::Table::from_iter([("path".to_owned(), toml::Value::from(path))]);
    if let Some(package) = package {
        dep.insert("package".into(), package.into());
    }
    toml::Value::Table(dep)
}

/// Points the staged crate's `horizon-mod-api` dependency at Horizon's own (`api`), whatever path
/// or version the mod's author used; its package name.
fn point_api_at(manifest: &Path, api: &str) -> Result<String, String> {
    let text = fs::read_to_string(manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let mut value: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", manifest.display()))?;
    let package = value
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| format!("{} has no [package] name", manifest.display()))?
        .to_owned();
    let deps = value
        .entry("dependencies")
        .or_insert(toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| format!("{}: [dependencies] is not a table", manifest.display()))?;
    deps.insert(crates::API_CRATE.into(), path_dep(api, None));
    write(
        manifest,
        &toml::to_string(&value).map_err(|e| e.to_string())?,
    )?;
    Ok(package)
}

fn main_rs(entries: &[String]) -> String {
    format!(
        "//! Generated by the launcher (native_mods/api.rs): the game with its API mods.\n\
         #![forbid(unsafe_code)]\n\n\
         #[global_allocator]\n\
         static ALLOCATOR: horizon_threads::Allocator = horizon_threads::Allocator;\n\n\
         static MOD_API_LEVEL: [u8; 27] = horizon_mods::api_level::MARK;\n\n\
         fn mods() -> horizon_mod_api::Mods {{\n    vec![\n{}\n    ]\n}}\n\n\
         fn main() {{\n    std::hint::black_box(&MOD_API_LEVEL);\n    \
         horizon_client::game::set_api_mods(mods);\n    horizon_client::entry::main();\n}}\n",
        entries.join("\n")
    )
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_mods_build_in_their_own_package_not_horizon_clients() {
        let root = std::env::temp_dir().join(format!("native-api-stage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let stage = root.join("stage");
        fs::create_dir_all(stage.join("crates/horizon-client")).unwrap();
        fs::write(
            stage.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/horizon-client\"]\n",
        )
        .unwrap();
        let client = "[package]\nname = \"horizon-client\"\n";
        fs::write(stage.join("crates/horizon-client/Cargo.toml"), client).unwrap();
        let dir = root.join("wand");
        fs::create_dir_all(dir.join("engine/src")).unwrap();
        fs::write(
            dir.join(MANIFEST),
            "[mod]\nid = \"wand\"\nversion = \"0.1.0\"\n[api]\ncrate = \"engine\"\nentry = \"create\"\n[[crate]]\nid = \"engine\"\npath = \"engine\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("engine/Cargo.toml"),
            "[package]\nname = \"wand-engine\"\nversion = \"0.1.0\"\n[dependencies]\nhorizon-mod-api = \"0.1\"\n",
        )
        .unwrap();
        fs::write(dir.join("engine/src/lib.rs"), "").unwrap();
        let bundle = super::super::bundle(&dir, None).unwrap();
        assert_eq!(bundle.api.as_ref().unwrap().entry, "create");
        stage_mods(&stage, &bundle);

        let read = |p: &str| fs::read_to_string(stage.join(p)).unwrap();
        let workspace: toml::Table = read("Cargo.toml").parse().unwrap();
        let members: Vec<_> = workspace["workspace"]["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap())
            .collect();
        assert_eq!(
            members,
            ["crates/horizon-client", "native-crates/wand/engine", GAME]
        );
        let game: toml::Table = read("native-game/Cargo.toml").parse().unwrap();
        assert_eq!(game["package"]["name"].as_str(), Some(PACKAGE));
        assert_eq!(game["bin"][0]["name"].as_str(), Some("horizon-client"));
        assert_eq!(
            game["dependencies"]["mod_wand"]["package"].as_str(),
            Some("wand-engine")
        );
        assert!(read("native-game/src/main.rs").contains("(\"wand\", mod_wand::create()),"));
        let engine: toml::Table = read("native-crates/wand/engine/Cargo.toml")
            .parse()
            .unwrap();
        assert_eq!(
            engine["dependencies"]["horizon-mod-api"]["path"].as_str(),
            Some("../../../crates/horizon-mod-api")
        );
        // horizon-client itself is untouched: it never depends on a mod.
        assert_eq!(read("crates/horizon-client/Cargo.toml"), client);
        fs::remove_dir_all(root).unwrap();
    }

    fn stage_mods(at: &Path, bundle: &Bundle) {
        stage(at, &[bundle]).unwrap();
    }
}
