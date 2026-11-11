//! Fire damage to a minecart.
use super::*;

impl Game<'_> {
    /// `BaseFireBlock.entityInside` for a cart: each fire block the cart's box
    /// reaches into hurts it with `inFire` for the block's `fireDamage`. Fire
    /// counts as a TNT minecart's ignition source; a cart has no `baseTick`,
    /// so the fire it is lit with never burns it further. The old behavior
    /// applies block effects twice a tick, each time at the position the cart
    /// has then and each block once per application: right after its move
    /// (`AbstractMinecart.move`) and at the end of the tick
    /// (`OldMinecartBehavior.tick`). The other behavior applies them once.
    pub(super) fn cart_fire_tick(&mut self, id: u32, slot: usize) {
        let Some(i) = cart_at(&self.carts, id, slot) else {
            return;
        };
        let e = f64::from(1.0e-5_f32);
        let end = self.carts[i].position;
        let mut spots: Vec<Vec3> = self.carts[i].fire_move_at.take().into_iter().collect();
        spots.push(end);
        let mut damages = Vec::new();
        for spot in spots {
            let (w, p) = (WIDTH / 2.0, spot);
            let b = Aabb::new(
                Vec3::new(p.x - w, p.y, p.z - w),
                Vec3::new(p.x + w, p.y + HEIGHT, p.z + w),
            )
            .deflate(Vec3::new(e, e, e));
            let (lo, hi) = (b.min_position(), b.max_position());
            for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
                for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                    for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                        if let Some(d) = self.fire_damage_at([x, y, z]) {
                            damages.push(d);
                        }
                    }
                }
            }
        }
        let invulnerable = self
            .persist
            .raw
            .get(&id)
            .is_some_and(|tag| horizon_client::entity_nbt::flag(tag, "Invulnerable"));
        if invulnerable || self.remote.is_some() {
            return;
        }
        for damage in damages {
            let Some(i) = cart_at(&self.carts, id, i) else {
                return;
            };
            self.hurt_cart(i, damage, false, true);
        }
    }
}
