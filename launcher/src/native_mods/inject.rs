//! Trait-backed HEAD injections, generated in the incoming copy and staged game.
use super::{api, Bundle, MANIFEST};
use quote::quote;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

/// A trait declaration to add to the staged game after horizon-mixins has run.
pub(super) struct Injection {
    name: syn::Ident,
    parameters: Vec<(syn::Ident, syn::Type)>,
    cancellable: bool,
}

/// Replace trait injections with ordinary mixin helpers, leaving ordinary injections unchanged.
pub(super) fn prepare(mods: &[Bundle], source: &Path) -> Result<Vec<Injection>, String> {
    let mut result = Vec::new();
    let mut names = HashSet::new();
    let owners: HashMap<_, _> = api::with_api(mods)
        .enumerate()
        .map(|(i, b)| (b.id.as_str(), i))
        .collect();
    // These are the copied bundles under mods.incoming, never the player's originals.
    for bundle in mods {
        api::strip(bundle)?;
        let path = bundle.dir.join(MANIFEST);
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut manifest: toml::Table = text
            .parse()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let entries = manifest
            .get_mut("injection")
            .and_then(toml::Value::as_array_mut);
        if let Some(entries) = entries {
            for (index, entry) in entries.iter_mut().enumerate() {
                let table = entry.as_table_mut().ok_or("injection must be a table")?;
                let Some(method) = table.remove("trait_method") else {
                    continue;
                };
                if bundle.api.is_none() {
                    return Err(format!("{}: trait_method requires [api]", path.display()));
                }
                let target = if let Some(hook) = table.get("hook").and_then(toml::Value::as_str) {
                    let catalog = source.join("native-hooks.json");
                    let text = fs::read_to_string(&catalog)
                        .map_err(|e| format!("{}: {e}", catalog.display()))?;
                    let catalog: serde_json::Value =
                        serde_json::from_str(&text).map_err(|e| e.to_string())?;
                    catalog["hooks"]
                        .as_array()
                        .and_then(|hooks| hooks.iter().find(|h| h["name"].as_str() == Some(hook)))
                        .and_then(|h| h["target"].as_str())
                        .ok_or_else(|| format!("unknown catalog hook {hook}"))?
                        .to_owned()
                } else {
                    table
                        .get("target")
                        .and_then(toml::Value::as_str)
                        .ok_or("trait_method needs a target or catalog hook")?
                        .to_owned()
                };
                let target_crate = target.split("::").next().unwrap_or_default();
                if target_crate != "horizon_client" {
                    return Err(format!("trait injection target {target:?} is in crate {target_crate:?}; only horizon-client targets are supported"));
                }
                if table.get("kind").and_then(toml::Value::as_str) != Some("HEAD") {
                    return Err(format!(
                        "{}: trait_method supports only HEAD injections",
                        path.display()
                    ));
                }
                if table.contains_key("helper") {
                    return Err(format!(
                        "{}: trait_method cannot also specify helper",
                        path.display()
                    ));
                }
                let method = method
                    .as_str()
                    .ok_or("trait_method must be a Rust identifier")?;
                let _: syn::Ident =
                    syn::parse_str(method).map_err(|_| "trait_method must be a Rust identifier")?;
                let name = format!("inject_{}_{}", bundle.id.replace('-', "_"), method);
                let ident: syn::Ident =
                    syn::parse_str(&name).map_err(|_| "invalid generated trait method")?;
                if !names.insert(name.clone()) {
                    return Err(format!("duplicate generated trait method {name}"));
                }
                let cancellable = match table.get("cancellable") {
                    None => false,
                    Some(v) => v.as_bool().ok_or("cancellable must be a boolean")?,
                };
                let mut parameters = Vec::new();
                let mut seen = HashSet::new();
                if let Some(bindings) = table.get("bindings") {
                    for binding in bindings.as_array().ok_or("bindings must be an array")? {
                        let binding = binding.as_table().ok_or("binding must be a table")?;
                        let name = binding
                            .get("name")
                            .and_then(toml::Value::as_str)
                            .ok_or("binding needs a name")?;
                        let ident: syn::Ident =
                            syn::parse_str(name).map_err(|_| "invalid binding name")?;
                        if !seen.insert(name.to_owned()) {
                            return Err(format!("duplicate binding {name}"));
                        }
                        let ty = binding
                            .get("ty")
                            .and_then(toml::Value::as_str)
                            .ok_or("binding needs a type")?;
                        let ty: syn::Type =
                            syn::parse_str(ty).map_err(|e| format!("invalid binding type: {e}"))?;
                        parameters.push((ident, ty));
                    }
                }
                let injection = Injection {
                    name: ident,
                    parameters,
                    cancellable,
                };
                let helper_path = format!("horizon-trait-injection-{index}.rs");
                let helper_file = bundle.dir.join(&helper_path);
                if helper_file.exists() {
                    return Err(format!(
                        "{} is reserved for generated helpers",
                        helper_file.display()
                    ));
                }
                fs::write(&helper_file, injection.helper(owners[bundle.id.as_str()]))
                    .map_err(|e| e.to_string())?;
                table.insert("helper".into(), helper_path.into());
                result.push(injection);
            }
        }
        fs::write(
            &path,
            toml::to_string(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(result)
}

impl Injection {
    fn helper(&self, owner: usize) -> String {
        let name = &self.name;
        let params = self.parameters.iter().map(|(n, t)| quote!(#n: #t));
        let args = self.parameters.iter().map(|(n, t)| {
            if matches!(t, syn::Type::Reference(r) if r.mutability.is_some()) {
                quote!(&mut *#n)
            } else {
                quote!(#n)
            }
        });
        let call = quote!(owner.#name(#(#args),*));
        // Before the mods are installed (an injection into code `main_with` runs first), or
        // with fewer mods than the build had, the injection does nothing.
        let (output, result, absent) = if self.cancellable {
            (
                quote!(-> core::ops::ControlFlow<()>),
                quote! {
                    if #call == crate::mod_api::Flow::Cancel {
                        return core::ops::ControlFlow::Break(());
                    }
                    core::ops::ControlFlow::Continue(())
                },
                quote!(core::ops::ControlFlow::Continue(())),
            )
        } else {
            (quote!(), quote!(#call;), quote!())
        };
        quote! {
            fn horizon_trait_dispatch(#(#params),*) #output {
                let Some(owner) = crate::mod_api::mods().get(#owner) else {
                    return #absent;
                };
                #result
            }
        }
        .to_string()
    }

    fn declaration(&self) -> String {
        let name = &self.name;
        let types = self.parameters.iter().map(|(_, ty)| ty);
        let (output, default) = if self.cancellable {
            (quote!(-> Flow), quote!(Flow::Continue))
        } else {
            (quote!(), quote!())
        };
        quote!(fn #name(&self, #(_: #types),*) #output { #default }).to_string()
    }
}

/// Add declarations to the existing generated trait, not its dispatch implementations.
pub(super) fn stage(stage: &Path, injections: &[Injection]) -> Result<(), String> {
    if injections.is_empty() {
        return Ok(());
    }
    let path = stage.join("crates/horizon-client/src/mod_api/generated.rs");
    let source = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = syn::parse_file(&source).map_err(|e| format!("{}: {e}", path.display()))?;
    let hooks = file
        .items
        .iter()
        .find_map(|i| match i {
            syn::Item::Trait(t) if t.ident == "Hooks" => Some(t),
            _ => None,
        })
        .ok_or("generated Hooks trait missing")?;
    for injection in injections {
        if hooks
            .items
            .iter()
            .any(|i| matches!(i, syn::TraitItem::Fn(f) if f.sig.ident == injection.name))
        {
            return Err(format!("trait method {} already exists", injection.name));
        }
    }
    let marker = "pub trait Hooks: Send + Sync {\n";
    if source.matches(marker).count() != 1 {
        return Err("generated Hooks marker missing or ambiguous".into());
    }
    let declarations = injections
        .iter()
        .map(Injection::declaration)
        .collect::<String>();
    fs::write(
        path,
        source.replacen(marker, &format!("{marker}{declarations}"), 1),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, injection: &str) -> (std::path::PathBuf, Bundle) {
        let root =
            std::env::temp_dir().join(format!("horizon-inject-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("engine/src")).unwrap();
        fs::write(
            root.join("engine/Cargo.toml"),
            "[package]\nname = 'mod-example'\nversion = '1.0.0'\n",
        )
        .unwrap();
        fs::write(root.join("engine/src/lib.rs"), "").unwrap();
        fs::write(
            root.join(MANIFEST),
            format!("[mod]\nid = 'example'\nversion = '1'\n[api]\npath = 'engine'\n{injection}"),
        )
        .unwrap();
        let bundle = super::super::bundle(&root, None).unwrap();
        (root, bundle)
    }

    #[test]
    fn owner_dispatch_and_cancel_are_generated_from_bindings() {
        let (root, bundle) = fixture("cancel", "[[injection]]\nid = 'hook'\ntarget = 'horizon_client::game::tick'\nkind = 'HEAD'\ntrait_method = 'tick'\ncancellable = true\nbindings = [{ name = 'amount', ty = '&mut i32', value = '&mut *amount' }]\n");
        let injections = prepare(&[bundle], &root).unwrap();
        let helper = fs::read_to_string(root.join("horizon-trait-injection-0.rs")).unwrap();
        let manifest = fs::read_to_string(root.join(MANIFEST)).unwrap();
        assert!(!manifest.contains("trait_method"));
        assert!(!manifest.contains("[api]"));
        let stage_dir = root.join("stage/crates/horizon-client/src/mod_api");
        fs::create_dir_all(&stage_dir).unwrap();
        fs::write(
            stage_dir.join("generated.rs"),
            "pub trait Hooks: Send + Sync {\n}\n",
        )
        .unwrap();
        stage(&root.join("stage"), &injections).unwrap();
        let output = fs::read_to_string(stage_dir.join("generated.rs")).unwrap();
        assert!(syn::parse_file(&output).is_ok());
        let program = format!(
            r#"
            mod mod_api {{
                use std::sync::OnceLock;
                #[derive(PartialEq)] pub enum Flow {{ Continue, Cancel }}
                {output}
                struct Owner;
                impl Hooks for Owner {{
                    fn inject_example_tick(&self, amount: &mut i32) -> Flow {{
                        *amount = 42;
                        Flow::Cancel
                    }}
                }}
                struct Other;
                impl Hooks for Other {{
                    fn inject_example_tick(&self, _: &mut i32) -> Flow {{ panic!("wrong owner") }}
                }}
                pub fn mods() -> &'static [Box<dyn Hooks>] {{
                    static MODS: OnceLock<Vec<Box<dyn Hooks>>> = OnceLock::new();
                    MODS.get_or_init(|| vec![Box::new(Owner), Box::new(Other)])
                }}
            }}
            {helper}
            fn main() {{
                let mut value = 1;
                assert!(matches!(horizon_trait_dispatch(&mut value), core::ops::ControlFlow::Break(())));
                assert_eq!(value, 42);
            }}
        "#
        );
        let source = root.join("proof.rs");
        let binary = root.join(format!("proof-bin{}", std::env::consts::EXE_SUFFIX));
        fs::write(&source, program).unwrap();
        let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let compiled = std::process::Command::new(compiler)
            .args(["--edition", "2024"])
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        assert!(std::process::Command::new(binary)
            .status()
            .unwrap()
            .success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn trait_edits_do_not_invalidate_mixin_cache() {
        let (root, bundle) = fixture("cache", "[[injection]]\nid = 'hook'\ntarget = 'horizon_client::game::tick'\nkind = 'HEAD'\ntrait_method = 'tick'\n");
        let key = || {
            super::super::inject_cache::key(
                "revision",
                "mixins",
                super::super::Profile::Fast,
                std::slice::from_ref(&bundle),
            )
            .unwrap()
        };
        let before = key();
        fs::write(root.join("engine/src/lib.rs"), "// one line edit").unwrap();
        assert_eq!(before, key());
        fs::write(root.join("other.rs"), "// mixin input").unwrap();
        assert_ne!(before, key());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mixin_inputs_inside_api_are_not_excluded_from_cache() {
        let (root, bundle) = fixture("api-helper", "[[injection]]\nid = 'hook'\ntarget = 'horizon_client::game::tick'\nkind = 'HEAD'\nhelper = 'engine/src/lib.rs'\n");
        let key = || {
            super::super::inject_cache::key(
                "revision",
                "mixins",
                super::super::Profile::Fast,
                std::slice::from_ref(&bundle),
            )
            .unwrap()
        };
        let before = key();
        fs::write(root.join("engine/src/lib.rs"), "fn changed() {}\n").unwrap();
        assert_ne!(before, key());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mixin_crate_containing_api_is_not_excluded_from_cache() {
        let (root, bundle) = fixture("api-parent", "[[crate]]\nid = 'whole_bundle'\npath = '.'\n");
        let key = || {
            super::super::inject_cache::key(
                "revision",
                "mixins",
                super::super::Profile::Fast,
                std::slice::from_ref(&bundle),
            )
            .unwrap()
        };
        let before = key();
        fs::write(root.join("engine/src/lib.rs"), "// changed\n").unwrap();
        assert_ne!(before, key());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_build_key_tracks_dispatch_order() {
        let (root, first) = fixture("order", "");
        let mut second = first.clone();
        second.id = "second".into();
        let a = super::super::version_for(
            "0123456789012345678901234567890123456789",
            "mixins",
            super::super::Profile::Fast,
            super::super::Target::Client,
            &[first.clone(), second.clone()],
        )
        .unwrap();
        let b = super::super::version_for(
            "0123456789012345678901234567890123456789",
            "mixins",
            super::super::Profile::Fast,
            super::super::Target::Client,
            &[second, first],
        )
        .unwrap();
        assert_ne!(a, b);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn trait_injections_reject_targets_in_other_crates() {
        let (root, bundle) = fixture("other-crate", "[[injection]]\nid = 'hook'\ntarget = 'horizon_world::loot::load'\nkind = 'HEAD'\ntrait_method = 'load'\n");
        let error = prepare(&[bundle], &root).err().unwrap();
        assert!(error.contains("horizon_world::loot::load") && error.contains("horizon_world"));
        fs::remove_dir_all(root).unwrap();

        let (root, bundle) = fixture("other-catalog", "[[injection]]\nid = 'hook'\nhook = 'loot_table_load'\nkind = 'HEAD'\ntrait_method = 'load'\n");
        fs::write(
            root.join("native-hooks.json"),
            r#"{"hooks":[{"name":"loot_table_load","target":"horizon_world::loot::load"}]}"#,
        )
        .unwrap();
        let error = prepare(&[bundle], &root).err().unwrap();
        assert!(error.contains("horizon_world::loot::load") && error.contains("horizon_world"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsupported_injection_is_rejected() {
        let (root, bundle) = fixture("return", "[[injection]]\nid = 'hook'\ntarget = 'horizon_client::game::tick'\nkind = 'RETURN'\ntrait_method = 'tick'\n");
        assert!(prepare(&[bundle], &root)
            .err()
            .unwrap()
            .contains("only HEAD"));
        fs::remove_dir_all(root).unwrap();
    }
}
