//! API mods (docs/native-mods.md, "API mods"): native mods without mixins, crates of their own using
//! `horizon-mod-api`. The executable of a build with them registers them (`set_api_mods`); each
//! event is asked right after the hook point it is named after, once the mixin mods there are done,
//! and only if none of them claimed or canceled it. A mod the player turned off is skipped, with no
//! rebuild (`native_on`).
use super::Game;
use horizon_client::{items::ItemStack, text::Text};
use horizon_mod_api::{Action, Host, Mod, Mods, first_handled};
use horizon_world::registry::BlockStateId;
use std::sync::OnceLock;

static REGISTER: OnceLock<fn() -> Mods> = OnceLock::new();
static MODS: OnceLock<Mods> = OnceLock::new();

/// Hands the game a build's API mods: called by its executable before `entry::main`.
pub fn set_api_mods(mods: fn() -> Mods) {
    let _ = REGISTER.set(mods);
}

fn mods() -> &'static Mods {
    MODS.get_or_init(|| REGISTER.get().map(|mods| mods()).unwrap_or_default())
}

impl Host for Game<'_> {
    fn block(&self, pos: [i32; 3]) -> String {
        self.native_block(pos)
    }
    fn set_block(&mut self, pos: [i32; 3], block: &str) -> bool {
        let Some(state) = self.pack.assets.registry.default_state(block) else {
            return false;
        };
        let dimension = self.dim.id().to_owned();
        self.native_set_block(
            &dimension,
            pos,
            state,
            super::native_hooks::flags::UPDATE_ALL,
        )
    }
    fn give_items(&mut self, item: &str, count: u32) -> u32 {
        self.native_give_items(item, count)
    }
    fn feedback(&mut self, text: &str) {
        self.acting_chat(Text::literal(text));
    }
}

impl Game<'_> {
    /// Asks the API mods the player left on, in order, until one handles the event.
    fn api(&mut self, mut ask: impl FnMut(&dyn Mod, &mut Self) -> Action) -> bool {
        let on: Vec<&dyn Mod> = mods()
            .iter()
            .filter(|(id, _)| self.native_on(id))
            .map(|(_, m)| m.as_ref())
            .collect();
        first_handled(on, |m| ask(m, self))
    }
    fn state_name(&self, state: BlockStateId) -> String {
        self.res.facts().get(state).name.to_string()
    }

    pub(super) fn api_use_item(&mut self, stack: &ItemStack, claimed: &mut bool) {
        *claimed = *claimed || self.api(|m, host| m.use_item(host, stack));
    }
    pub(super) fn api_use_item_on(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
        claimed: &mut bool,
    ) {
        *claimed = *claimed || self.api(|m, host| m.use_item_on(host, stack, pos, face));
    }
    pub(super) fn api_right_click_block(
        &mut self,
        pos: [i32; 3],
        face: [i32; 3],
        canceled: &mut bool,
    ) {
        *canceled = *canceled || self.api(|m, host| m.right_click_block(host, pos, face));
    }
    pub(super) fn api_left_click_block(
        &mut self,
        pos: [i32; 3],
        face: [i32; 3],
        state: BlockStateId,
        canceled: &mut bool,
    ) {
        let block = self.state_name(state);
        *canceled = *canceled || self.api(|m, host| m.left_click_block(host, pos, face, &block));
    }
    pub(super) fn api_destroy_block(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        canceled: &mut bool,
    ) {
        let block = self.state_name(state);
        *canceled = *canceled || self.api(|m, host| m.destroy_block(host, pos, &block));
    }
    pub(super) fn api_place_block(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        state: BlockStateId,
    ) {
        let block = self.state_name(state);
        self.api(|m, host| {
            m.place_block(host, stack, pos, &block);
            Action::Pass
        });
    }
    /// Options > Mods' rows for the API mods, as native mods': the switch turns one off at once.
    pub(super) fn api_mod_rows(&self, rows: &mut Vec<super::native_hooks::NativeMod>) {
        rows.extend(mods().iter().map(|(id, _)| super::native_hooks::NativeMod {
            id: (*id).to_owned(),
            title: (*id).to_owned(),
            description: "API mod (no mixins)".into(),
            keys: Vec::new(),
        }));
    }
    pub(super) fn api_tooltip(&self, stack: &ItemStack, lines: &mut Vec<(Text, u32)>) {
        for (id, m) in mods() {
            if self.native_on(id) {
                let mut added = Vec::new();
                m.tooltip(stack, &mut added);
                lines.extend(added.into_iter().map(|l| (Text::literal(l), u32::MAX)));
            }
        }
    }
}
