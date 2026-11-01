//! Entities ticking for native mods (`native_entity_tick_pre`, `native_entity_tick_post`:
//! NeoForge's `EntityTickEvent.Pre` and `.Post`). A family of entities ticks in its own loop,
//! and the loops of the mobs and the items hold their lists out of `Game`, so a family hands
//! its ticking entities to a `TickRound` before the loop and hears the Posts after it; the
//! minecarts and boats, whose loops can call the hooks, ask for each one in turn.
use super::Game;
use horizon_client::mobs::MobType;
use std::sync::Arc;

/// The type of an entity in a round, as the hooks name it.
pub(super) enum TickKind {
    Fixed(&'static str),
    Mob(Arc<MobType>),
}

impl TickKind {
    fn id(&self) -> &str {
        match self {
            Self::Fixed(id) => id,
            Self::Mob(kind) => &kind.id,
        }
    }
}

/// The entities of one family in one tick.
#[derive(Default)]
pub(super) struct TickRound {
    /// Whether this game is a server the mods hear: otherwise nothing is told or kept.
    listening: bool,
    /// The entities the Pre let through, in order, for the Post.
    ticking: Vec<(u32, TickKind)>,
    /// The entities a Pre canceled.
    canceled: Vec<u32>,
}

impl TickRound {
    /// Whether a mod canceled the tick of entity `id`.
    pub(super) fn canceled(&self, id: u32) -> bool {
        !self.canceled.is_empty() && self.canceled.contains(&id)
    }
    /// Whether the mods hear this round.
    pub(super) fn listening(&self) -> bool {
        self.listening
    }
}

impl Game<'_> {
    /// A round for a family about to tick: on a server only (the server thread or a dedicated
    /// server, never a client nor the deprecated in-frame path). Its list is kept between ticks.
    pub(super) fn native_tick_round(&mut self) -> TickRound {
        let listening = self.serving.is_some() || self.options.dedicated.is_some();
        TickRound {
            listening,
            ticking: if listening {
                std::mem::take(&mut self.creatures.tick_buffer)
            } else {
                Vec::new()
            },
            canceled: Vec::new(),
        }
    }

    /// `native_entity_tick_pre` for the entity `id` of `kind` about to tick in a family that
    /// hears its Posts together: whether it ticks.
    pub(super) fn native_tick_pre_batch(
        &mut self,
        round: &mut TickRound,
        id: u32,
        kind: TickKind,
    ) -> bool {
        if !round.listening {
            return true;
        }
        let mut canceled = false;
        self.native_entity_tick_pre(self.dim.id(), id, kind.id(), &mut canceled);
        if canceled {
            round.canceled.push(id);
        } else {
            round.ticking.push((id, kind));
        }
        !canceled
    }

    /// `native_entity_tick_post` for each entity the round let through, and the round is over.
    pub(super) fn native_tick_post_batch(&mut self, mut round: TickRound) {
        if !round.listening {
            return;
        }
        let dimension = self.dim.id();
        for (id, kind) in &round.ticking {
            self.native_entity_tick_post(dimension, *id, kind.id());
        }
        round.ticking.clear();
        self.creatures.tick_buffer = round.ticking;
    }

    /// `native_entity_tick_pre` for one entity about to tick: whether it does.
    pub(super) fn native_tick_pre_one(
        &mut self,
        round: &mut TickRound,
        id: u32,
        kind: &str,
    ) -> bool {
        if !round.listening {
            return true;
        }
        let mut canceled = false;
        self.native_entity_tick_pre(self.dim.id(), id, kind, &mut canceled);
        if canceled {
            round.canceled.push(id);
        }
        !canceled
    }

    /// `native_entity_tick_post` for one entity that ticked.
    pub(super) fn native_tick_post_one(&mut self, round: &TickRound, id: u32, kind: &str) {
        if round.listening {
            self.native_entity_tick_post(self.dim.id(), id, kind);
        }
    }
}
