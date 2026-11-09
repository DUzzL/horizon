//! The server player's impulse context (wind charge, mace launch) over a
//! tick of accepted moves: `ServerGamePacketListenerImpl.handlePlayerPositionChange`
//! ends it, `LivingEntity.tick` counts its grace time down.
use crate::motion::{MotionWorld, PlayerMotion};

/// What `handlePlayerPositionChange` reads before it moves the player.
pub struct MoveStart {
    pub grounded: bool,
    gliding: bool,
    spin_attacking: bool,
}

impl MoveStart {
    pub fn of(w: &MotionWorld<'_, '_, '_>) -> Self {
        Self {
            grounded: false,
            gliding: w.player.fall_flying,
            spin_attacking: w.player.auto_spin_attack_ticks > 0,
        }
    }
}

/// After the tick's moves: a liquid landing (`hasLandedInLiquid`: a move into
/// a liquid that does not rise), the ground, a climbable, gliding or a spin
/// attack end the context, still deferred by the grace time; then the grace
/// time counts down.
pub fn end_after_moves(w: &mut MotionWorld<'_, '_, '_>, start: &MoveStart) {
    let liquid = w.player.fluids.was_touching_water || w.player.is_in_lava();
    let delta_y = w.player.velocity.y;
    w.player.impulse.landed_in_liquid(delta_y, liquid);
    let climbing = w.climbable().unwrap_or(false);
    w.player
        .impulse
        .after_move(start.grounded || climbing || start.gliding || start.spin_attacking);
    w.player.impulse.tick();
}

/// A spectator's accepted moves end the context; the grace time counts down.
pub fn spectator_tick(player: Option<&mut PlayerMotion>, moved: bool) {
    if let Some(p) = player {
        p.impulse.after_move(moved);
        p.impulse.tick();
    }
}
