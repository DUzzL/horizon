use super::*;

/// `Villager.readAdditionalSaveData`: a tag with `VillagerData` or
/// `VillagerDataFinalized: 1b` finalizes; one with neither stays false.
#[test]
fn a_villagers_finalized_flag_follows_its_save() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    if !root.join("items.json").exists() {
        return;
    }
    let data = crate::mobs::MobTypes::load(&root).unwrap();
    let types: HashMap<String, Arc<MobType>> = data
        .types
        .iter()
        .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
        .collect();
    let base = || {
        vec![
            ("id", string("minecraft:villager")),
            ("Pos", doubles(&[0.5, 64.0, 0.5])),
        ]
    };
    let flag_of = |fields: Vec<(&'static str, Tag)>| {
        let m = mob_from_nbt(&Tag::compound(fields), &types, 1, None).unwrap();
        let tag = mob_to_nbt(&m, [1, 2, 3, 4], None, None, |_| None);
        number(tag.get("VillagerDataFinalized"))
    };
    assert_eq!(flag_of(base()), Some(0));
    let mut with_flag = base();
    with_flag.push(("VillagerDataFinalized", Tag::Byte(1)));
    assert_eq!(flag_of(with_flag), Some(1));
    let mut with_data = base();
    with_data.push(("VillagerData", Tag::compound([("level", Tag::Int(1))])));
    assert_eq!(flag_of(with_data), Some(1));
}
