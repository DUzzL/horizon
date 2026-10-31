//! The API of a native mod without mixins (docs/native-mods.md, "API mods"). The mod is a crate of
//! its own, `"api"` in its `horizon-native.json`: the game calls it right after the hook points of
//! `native-hooks.json` its events are named after, so building or changing it compiles that crate
//! and links the game, never horizon-client itself.
//!
//! Each event runs server-side for every player of the world (the acting one is the `Host`'s),
//! except `tooltip`, which runs where the tooltip shows. A mixin mod hooking the same point runs
//! first: an event it already claimed or canceled doesn't reach the API mods.
pub use horizon_items::items::ItemStack;

/// The mods of a build, by mod id: what the generated executable hands the game.
pub type Mods = Vec<(&'static str, Box<dyn Mod>)>;

/// A mod's answer to an event the game lets it take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Action {
    /// The game, and the mods after this one, go on as without it.
    #[default]
    Pass,
    /// Taken: the hook's `claimed` (a use) or `canceled` (a click, a break) is set, and the mods
    /// after this one aren't asked.
    Handled,
}

/// The game as an event sees it: the acting player's world, on the server.
pub trait Host {
    /// The block at `pos` (`mymod:cable`; air where nothing is loaded).
    fn block(&self, pos: [i32; 3]) -> String;
    /// Sets the block at `pos` to `block`'s default state with neighbour updates; false for an
    /// unknown block, no change, or a client.
    fn set_block(&mut self, pos: [i32; 3], block: &str) -> bool;
    /// Gives up to `count` plain `item` where they fit; what it gave (0 for an unknown item).
    fn give_items(&mut self, item: &str, count: u32) -> u32;
    /// A chat line to the acting player.
    fn feedback(&mut self, text: &str);
}

/// A mod: implemented by a normal Rust library, created by the public function its manifest's
/// `"entry"` names. Every event has a default that does nothing.
pub trait Mod: Send + Sync {
    /// `use_item`: a player uses `stack` (right click), in the air or on a block nothing took.
    fn use_item(&self, _host: &mut dyn Host, _stack: &ItemStack) -> Action {
        Action::Pass
    }
    /// `use_item_on`: a player uses `stack` on the block at `pos`, `face` the face clicked
    /// (`[0, 1, 0]` is the top). `Handled` also stops a block item's placement.
    fn use_item_on(
        &self,
        _host: &mut dyn Host,
        _stack: &ItemStack,
        _pos: [i32; 3],
        _face: [i32; 3],
    ) -> Action {
        Action::Pass
    }
    /// `right_click_block`: before the block's own use (a chest, a door) and `use_item_on`.
    fn right_click_block(&self, _host: &mut dyn Host, _pos: [i32; 3], _face: [i32; 3]) -> Action {
        Action::Pass
    }
    /// `left_click_block`: the start of digging `block` at `pos`.
    fn left_click_block(
        &self,
        _host: &mut dyn Host,
        _pos: [i32; 3],
        _face: [i32; 3],
        _block: &str,
    ) -> Action {
        Action::Pass
    }
    /// `destroy_block`: a player breaks `block` at `pos`; `Handled` keeps it.
    fn destroy_block(&self, _host: &mut dyn Host, _pos: [i32; 3], _block: &str) -> Action {
        Action::Pass
    }
    /// `place_block`: a player placed `stack` as `block` at `pos`. It only hears.
    fn place_block(&self, _host: &mut dyn Host, _stack: &ItemStack, _pos: [i32; 3], _block: &str) {}
    /// `tooltip`: lines to add under `stack`'s tooltip, where it shows.
    fn tooltip(&self, _stack: &ItemStack, _lines: &mut Vec<String>) {}
}

/// Asks `mods` in order until one handles the event: whether one did.
pub fn first_handled<'a>(
    mods: impl IntoIterator<Item = &'a dyn Mod>,
    mut ask: impl FnMut(&dyn Mod) -> Action,
) -> bool {
    mods.into_iter().any(|m| ask(m) == Action::Handled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Wand(AtomicU32);
    impl Mod for Wand {
        fn use_item(&self, host: &mut dyn Host, stack: &ItemStack) -> Action {
            self.0.fetch_add(1, Ordering::Relaxed);
            if &*stack.item != "mymod:wand" {
                return Action::Pass;
            }
            host.feedback("wand used");
            Action::Handled
        }
    }
    struct Quiet;
    impl Mod for Quiet {}

    #[derive(Default)]
    struct World(Vec<String>);
    impl Host for World {
        fn block(&self, _: [i32; 3]) -> String {
            "minecraft:air".into()
        }
        fn set_block(&mut self, _: [i32; 3], _: &str) -> bool {
            false
        }
        fn give_items(&mut self, _: &str, _: u32) -> u32 {
            0
        }
        fn feedback(&mut self, text: &str) {
            self.0.push(text.into());
        }
    }

    fn stack(item: &str) -> ItemStack {
        ItemStack {
            item: item.into(),
            count: 1,
            damage: 0,
            extra: None,
        }
    }

    #[test]
    fn events_pass_until_a_mod_handles_them() {
        let (quiet, wand, after) = (Quiet, Wand(AtomicU32::new(0)), Wand(AtomicU32::new(0)));
        let mods: [&dyn Mod; 3] = [&quiet, &wand, &after];
        let mut world = World::default();
        let wand_stack = stack("mymod:wand");
        assert!(first_handled(mods, |m| m.use_item(&mut world, &wand_stack)));
        assert_eq!(world.0, ["wand used"]);
        // The mod after the one that handled it is never asked.
        assert_eq!(after.0.load(Ordering::Relaxed), 0);
        let stick = stack("minecraft:stick");
        assert!(!first_handled(mods, |m| m.use_item(&mut world, &stick)));
        assert_eq!(after.0.load(Ordering::Relaxed), 1);
        assert_eq!(
            quiet.destroy_block(&mut world, [0, 64, 0], "minecraft:stone"),
            Action::Pass
        );
    }
}
