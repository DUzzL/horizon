//! API Demo: a stick that answers when used, and a line under its tooltip.
use horizon_mod_api::{Action, Host, ItemStack, Mod};

pub fn create() -> Box<dyn Mod> {
    Box::new(Demo)
}

struct Demo;

impl Mod for Demo {
    fn use_item(&self, host: &mut dyn Host, stack: &ItemStack) -> Action {
        if &*stack.item != "minecraft:stick" {
            return Action::Pass;
        }
        host.feedback("API Demo: the stick works");
        Action::Handled
    }

    fn place_block(&self, host: &mut dyn Host, _stack: &ItemStack, pos: [i32; 3], block: &str) {
        if block == "minecraft:diamond_block" {
            host.feedback(&format!("API Demo: diamond block at {pos:?}"));
        }
    }

    fn tooltip(&self, stack: &ItemStack, lines: &mut Vec<String>) {
        if &*stack.item == "minecraft:stick" {
            lines.push("Used by API Demo".into());
        }
    }
}
