"""Exercise the server orb pickup dispatch with a fake native mod."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ExperiencePickupHookTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("rustc"), "requires Rust")
    def test_cancel_and_mutate_merged_orb(self):
        source = (ROOT / "crates/horizon-client/src/game/lan_actions.rs").read_text()
        start = source.index("        let index = touching[pick];", source.index("fn guest_orbs("))
        end = source.index("        host.hub.take_item(", start)
        dispatch = source[start:end]
        fixture = r'''
#[derive(Clone, Debug, PartialEq)]
struct Orb { id: u32, value: u32, count: u32 }
struct Dim;
impl Dim { fn id(&self) -> &'static str { "overworld" } }
#[derive(Default)] struct Play { xp_delay: u32 }
#[derive(Default)] struct Host { play: std::collections::HashMap<u32, Play> }
struct Options { dedicated: Option<()> }
struct Game { orbs: Vec<Orb>, serving: Option<()>, options: Options, dim: Dim,
              cancel: bool, new_value: u32, new_count: u32, calls: u32 }
impl Game {
    fn as_guest<R>(&mut self, _: &mut Host, _: u32, f: impl FnOnce(&mut Self) -> R) -> Option<R> { Some(f(self)) }
    fn native_acting_id(&mut self) -> u128 { 123 }
    fn native_experience_orb_pickup(&mut self, player: u128, dimension: &str, id: u32,
                                    value: &mut u32, count: &mut u32, canceled: &mut bool) {
        assert_eq!((player, dimension, id, *value, *count), (123, "overworld", 7, 3, 4));
        self.calls += 1; *value = self.new_value; *count = self.new_count; *canceled = self.cancel;
    }
    fn remove_orb(&mut self, i: usize) { self.orbs.remove(i); }
    fn pickup(&mut self, host: &mut Host) -> Option<Orb> {
        let conn = 5; let touching = [0]; let pick = 0;
        DISPATCH
        Some(orb)
    }
}
fn game(cancel: bool, value: u32, count: u32) -> Game {
    Game { orbs: vec![Orb { id: 7, value: 3, count: 4 }], serving: Some(()),
           options: Options { dedicated: None }, dim: Dim, cancel, new_value: value,
           new_count: count, calls: 0 }
}
#[test] fn canceled_keeps_orb_and_delay() {
    let mut g = game(true, 10, 9); let mut h = Host::default();
    assert_eq!(g.pickup(&mut h), None);
    assert_eq!(g.orbs[0], Orb { id: 7, value: 3, count: 4 });
    assert!(h.play.is_empty()); assert_eq!(g.calls, 1);
}
#[test] fn mutations_persist_and_one_count_is_taken() {
    let mut g = game(false, 10, 9); let mut h = Host::default();
    assert_eq!(g.pickup(&mut h).unwrap().value, 10);
    assert_eq!(g.orbs[0], Orb { id: 7, value: 10, count: 8 });
    assert_eq!(h.play[&5].xp_delay, 2);
    let mut g = game(false, 0, 0); let mut h = Host::default();
    assert_eq!(g.pickup(&mut h).unwrap().value, 1); assert!(g.orbs.is_empty());
}
'''.replace("DISPATCH", dispatch.replace("return;", "return None;"))
        with tempfile.TemporaryDirectory() as folder:
            src, exe = Path(folder) / "pickup.rs", Path(folder) / "pickup"
            src.write_text(fixture)
            subprocess.run(["rustc", "--edition=2024", "--test", str(src), "-o", str(exe)], check=True)
            subprocess.run([str(exe)], check=True)

    @unittest.skipUnless(shutil.which("rustc"), "requires Rust")
    def test_mending_selected_item_and_amount(self):
        from test_world_input_capture import method
        source = (ROOT / "crates/horizon-client/src/game/enchant_effects.rs").read_text()
        actual = method(source.replace("pub(super) fn mending", "fn mending"), "mending")
        fixture = r'''
#[derive(Clone)] struct ItemStack { damage: u32, id: u32 }
impl ItemStack { fn enchantment_level(&self, _: &str) -> u32 { 1 } }
#[derive(Clone, Copy)] enum MendSlot { Main, Off, Armor(usize) }
#[derive(Default)] struct EffectContext { items: Option<()> }
struct Random;
impl Random { fn next_int_bound(&mut self, _: i32) -> Option<i32> { Some(1) } }
struct World { random: Random }
struct Enchantments;
impl Enchantments { fn modify_value(&self, _: Option<&ItemStack>, _: &str, ctx: &EffectContext,
                                    _: &mut Random, amount: f32) -> f32 {
    let _ = ctx.items; amount * 2.0
} }
struct Stations { enchantments: Enchantments }
macro_rules! world_items { ($s:expr) => { () } }
macro_rules! world_stations { ($s:expr) => { Stations { enchantments: Enchantments } } }
struct Options { dedicated: Option<()> }
struct Game { world: World, armor: Vec<Option<ItemStack>>, main: Option<ItemStack>, off: Option<ItemStack>,
              serving: Option<()>, options: Options, seen: Vec<(u32, u32, u32)> }
impl Game {
    fn mend_slot(&self, slot: MendSlot) -> &Option<ItemStack> { match slot { MendSlot::Main => &self.main, MendSlot::Off => &self.off, MendSlot::Armor(i) => &self.armor[i] } }
    fn mend_slot_mut(&mut self, slot: MendSlot) -> &mut Option<ItemStack> { match slot { MendSlot::Main => &mut self.main, MendSlot::Off => &mut self.off, MendSlot::Armor(i) => &mut self.armor[i] } }
    fn native_acting_id(&mut self) -> u128 { 123 }
    fn native_mending_repair(&mut self, player: u128, stack: &ItemStack, xp: u32, repair: &mut u32) {
        assert_eq!(player, 123); self.seen.push((stack.id, xp, *repair)); *repair = 4;
    }
    METHOD
}
#[test] fn selected_item_and_modified_repair() {
    let mut g = Game { world: World { random: Random }, armor: vec![],
        main: Some(ItemStack { id: 1, damage: 10 }), off: Some(ItemStack { id: 2, damage: 10 }),
        serving: Some(()), options: Options { dedicated: None }, seen: vec![] };
    assert_eq!(g.mending(3), 0);
    assert_eq!(g.seen, vec![(2, 3, 6)]);
    assert_eq!(g.main.unwrap().damage, 10); assert_eq!(g.off.unwrap().damage, 6);
}
'''.replace("METHOD", actual)
        with tempfile.TemporaryDirectory() as folder:
            src, exe = Path(folder) / "mending.rs", Path(folder) / "mending"
            src.write_text(fixture)
            subprocess.run(["rustc", "--edition=2024", "--test", str(src), "-o", str(exe)], check=True)
            subprocess.run([str(exe)], check=True)
