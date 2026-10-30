//! Boats, minecarts, item frames, armor stands and paintings for a server's
//! client: single player on its server thread, a LAN guest, a player on a
//! dedicated server.
//!
//! The client aims at them like at mobs (`Minecraft.hitResult`, then
//! `ServerboundInteractPacket`), and rides the vehicle the server seats it in
//! (`ClientPacketListener.handleSetEntityPassengersPacket`). A boat is
//! controlled by its rider's client: it ticks here and its moves go to the
//! server (`LocalPlayer.tick`: `ServerboundMoveVehiclePacket`,
//! `AbstractBoat.tick`: `ServerboundPaddleBoatPacket`). A minecart moves on
//! the server with the rider's input, and the rider sits in it.
use super::{Game, Keys, ray_box};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_protocol::packets::play::{self, Serverbound as S};

/// An object under the crosshair: its entity id, how far along the view it
/// was hit, and where (`EntityHitResult.getLocation` minus the entity's
/// position, the interact packet's location).
pub(super) struct ObjectHit {
    pub id: i32,
    pub distance: f64,
    pub location: [f64; 3],
}

impl Game<'_> {
    /// The nearest boat, minecart, item frame, armor stand or painting the
    /// view hits within reach; also End crystals when attacking.
    pub(super) fn remote_object_hit(&self, attacking: bool) -> Option<ObjectHit> {
        let (eye, dir) = self.aim()?;
        let at = |d: f64| {
            [
                eye[0] + dir[0] * d,
                eye[1] + dir[1] * d,
                eye[2] + dir[2] * d,
            ]
        };
        let block_centre = |p: [i32; 3]| {
            [
                f64::from(p[0]) + 0.5,
                f64::from(p[1]) + 0.5,
                f64::from(p[2]) + 0.5,
            ]
        };
        let mut hits: Vec<(i32, f64, [f64; 3])> = Vec::new();
        if let Some((i, d)) = self.painting_hit() {
            let p = &self.paintings[i];
            hits.push((p.id as i32, d, block_centre(p.pos)));
        }
        if let Some((i, d)) = self.frame_hit() {
            let f = &self.item_frames[i];
            hits.push((f.id as i32, d, block_centre(f.pos)));
        }
        if let Some((i, d, _)) = self.armor_stand_hit() {
            let a = &self.armor_stands[i];
            let p = a.position;
            hits.push((a.id as i32, d, [p.x, p.y, p.z]));
        }
        if let Some((i, d, _)) = self.cushion_hit() {
            let c = &self.cushions[i];
            let p = c.position;
            hits.push((c.id as i32, d, [p.x, p.y, p.z]));
        }
        if let Some((i, d)) = self.boat_hit() {
            let p = self.boats[i].position;
            hits.push((self.boats[i].id as i32, d, [p.x, p.y, p.z]));
        }
        if let Some((i, d)) = self.cart_hit() {
            let p = self.carts[i].position;
            hits.push((self.carts[i].id as i32, d, [p.x, p.y, p.z]));
        }
        if attacking {
            // EndCrystal.isPickable: its 2 x 2 box, with the server's entity
            // id, not special_targets' local projectile target id.
            // Entity.interact passes on a crystal, so right clicks use the item.
            let reach = if self.creative { 5.0 } else { 3.0 };
            for c in self.crystals.iter().filter(|c| c.id != 0) {
                let p = c.position;
                let bounds = Aabb::new(
                    Vec3::new(p.x - 1.0, p.y, p.z - 1.0),
                    Vec3::new(p.x + 1.0, p.y + 2.0, p.z + 1.0),
                );
                if let Some(d) = ray_box(eye, dir, reach, bounds) {
                    hits.push((c.id as i32, d, [p.x, p.y, p.z]));
                }
            }
            // EnderDragonPart.isPickable: the server's dragon hurts by part.
            hits.extend(self.remote_dragon_part_hit(eye, dir, reach));
        }
        let (id, distance, origin) = hits.into_iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
        let point = at(distance);
        Some(ObjectHit {
            id,
            distance,
            location: [
                point[0] - origin[0],
                point[1] - origin[1],
                point[2] - origin[2],
            ],
        })
    }

    /// `ClientPacketListener.handleSetEntityPassengersPacket`: who rides
    /// `vehicle` now. The local player gets in or out; mobs and other
    /// players sit where the server keeps them.
    pub(super) fn remote_set_passengers(&mut self, vehicle: i32, passengers: &[i32]) {
        let Some(me) = self.remote.as_ref().map(|r| r.entity_id) else {
            return;
        };
        let vehicle_id = vehicle as u32;
        for m in &mut self.mobs {
            if m.extra.mount == Some(vehicle_id) && !passengers.contains(&(m.id as i32)) {
                m.extra.mount = None;
                m.extra.passenger = false;
            } else if passengers.contains(&(m.id as i32)) {
                m.extra.mount = Some(vehicle_id);
                m.extra.passenger = true;
            }
        }
        for (id, display) in &mut self.text_displays {
            if passengers.contains(id) {
                display.vehicle = Some(vehicle_id);
            } else if display.vehicle == Some(vehicle_id) {
                display.vehicle = None;
            }
        }
        if let Some(r) = self.remote.as_mut() {
            r.first_passenger = passengers.first() == Some(&me);
        }
        if passengers.contains(&me) {
            if self.riding != Some(vehicle_id) {
                self.riding = Some(vehicle_id);
                if let Some(p) = self.player.as_mut() {
                    p.velocity = Vec3::ZERO;
                    p.fall_distance = 0.0;
                }
            }
        } else if self.riding == Some(vehicle_id) {
            // The server takes the player out, then teleports it.
            self.riding = None;
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == vehicle_id) {
                m.rider = None;
            }
            // HappyGhast.removePassenger: a still timeout of 10 (the server's
            // STAYS_STILL confirms it), and the rider lands on top of the ghast
            // (`place_ghast_under`).
            if let Some(m) = self.mobs.iter_mut().find(|m| {
                m.id == vehicle_id && m.species() == horizon_client::mobs::Species::HappyGhast
            }) {
                m.extra.counter = 10;
                if let Some(r) = self.remote.as_mut() {
                    r.ghast_dismount = Some(vehicle_id);
                }
            }
        }
    }

    /// The local player's tick in its vehicle, in place of its own movement:
    /// false when it rides nothing.
    pub(super) fn remote_ride(&mut self, keys: Keys) -> bool {
        let Some(id) = self.riding else {
            return false;
        };
        if let Some(i) = self.mobs.iter().position(|m| m.id == id) {
            // An animal moves on the server with the rider's input
            // (PlayerInput); here the jump bar charges for the HUD and the
            // rider sits where `positionRider` puts it.
            self.charge_mount_jump(i, keys);
            self.drive_remote_ghast(i, keys);
            self.position_rider();
            return true;
        }
        let seat = if let Some(c) = self.cushions.iter().find(|c| c.id == id) {
            // The seat does not move; the rider sits on the cushion.
            Some(c.seat())
        } else if let Some(i) = self.boats.iter().position(|b| b.id == id) {
            // AbstractBoat.tick on the controlling client: steer and move.
            let mut b = self.boats[i].clone();
            b.input = [keys.left, keys.right, keys.forward, keys.backward];
            let b = self.tick_boat(b, true);
            // positionRider: the seat turns with the boat; clampRotation.
            self.yaw += b.delta_rotation;
            let delta = (self.yaw - b.yaw + 180.0).rem_euclid(360.0) - 180.0;
            self.yaw += delta.clamp(-105.0, 105.0) - delta;
            self.boats[i] = b;
            // In front of a mob that shares the boat.
            Some(self.player_boat_seat(i))
        } else {
            // A minecart: the server moves it, the rider sits in it.
            self.carts.iter().find(|c| c.id == id).map(|c| {
                c.position
                    .add_vector(Vec3::new(0.0, super::minecarts::SEAT, 0.0))
            })
        };
        if let Some(p) = self.player.as_mut() {
            self.previous_position = p.position;
            if let Some(v) = seat {
                p.position = v;
            }
            p.velocity = Vec3::ZERO;
            p.fall_distance = 0.0;
        }
        true
    }

    /// `LocalPlayer.tick` for a passenger: the boat it controls goes to the
    /// server every tick with its paddles (the rider's own position does not).
    pub(super) fn remote_vehicle_packets(&self) -> Vec<S> {
        if let Some(m) = self.driven_mob() {
            // A mob its rider steers moves on this client, which reports it.
            return vec![S::MoveVehicle(play::MoveVehicleRequest {
                position: [m.position.x, m.position.y, m.position.z],
                yaw: m.yaw,
                pitch: m.head_pitch,
                on_ground: m.on_ground,
            })];
        }
        let Some(b) = self
            .riding
            .and_then(|id| self.boats.iter().find(|b| b.id == id))
        else {
            return Vec::new();
        };
        vec![
            S::MoveVehicle(play::MoveVehicleRequest {
                position: [b.position.x, b.position.y, b.position.z],
                yaw: b.yaw,
                pitch: 0.0,
                on_ground: b.on_ground,
            }),
            S::PaddleBoat(play::PaddleBoat {
                left: b.paddling[0],
                right: b.paddling[1],
            }),
        ]
    }

    /// `ClientPacketListener.handleMoveVehicle`: the server put the boat the
    /// player drives back.
    pub(super) fn remote_move_vehicle(&mut self, position: [f64; 3], yaw: f32) {
        if let Some(id) = self.driven_mob().map(|m| m.id)
            && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
        {
            let at = Vec3::new(position[0], position[1], position[2]);
            m.previous = at;
            m.position = at;
            m.yaw = yaw;
            m.previous_yaw = yaw;
            m.velocity = Vec3::ZERO;
            return;
        }
        let Some(b) = self
            .riding
            .and_then(|id| self.boats.iter_mut().find(|b| b.id == id))
        else {
            return;
        };
        let at = Vec3::new(position[0], position[1], position[2]);
        b.previous = at;
        b.position = at;
        b.yaw = yaw;
        b.previous_yaw = yaw;
        b.velocity = Vec3::ZERO;
    }

    /// The mob the player rides while this client moves it: on a server, a
    /// harnessed happy ghast is controlled by its rider's client
    /// (`isLocalInstanceAuthoritative`), which sends its moves
    /// (`ServerboundMoveVehiclePacket`) instead of the server steering it.
    pub(super) fn driven_mob(&self) -> Option<&horizon_client::mobs::Mob> {
        let id = self.riding?;
        if !self.remote.as_ref()?.first_passenger {
            return None;
        }
        self.mobs.iter().find(|m| {
            m.id == id
                && m.species() == horizon_client::mobs::Species::HappyGhast
                && horizon_client::mobs::equine::controlled(m)
        })
    }

    /// `LocalPlayer.aiStep` and `HappyGhast.tickRidden` on the controlling
    /// client: hand the rider's input to the ghast and tick it here.
    fn drive_remote_ghast(&mut self, i: usize, keys: Keys) {
        use horizon_client::mobs::{Species, equine};
        if self.mobs[i].species() != Species::HappyGhast {
            return;
        }
        // Only the first passenger steers; the others see the server's ghast.
        if !self.remote.as_ref().is_some_and(|r| r.first_passenger) {
            self.mobs[i].rider = None;
            return;
        }
        let strafe = f32::from(u8::from(keys.left)) - f32::from(u8::from(keys.right));
        let forward = f32::from(u8::from(keys.forward)) - f32::from(u8::from(keys.backward));
        self.mobs[i].rider = Some(equine::Rider {
            strafe,
            forward,
            yaw: self.yaw,
            pitch: self.pitch,
            jump: 0.0,
            steer: false,
            sprint: keys.sprint,
            jumping: keys.jump,
        });
        if !equine::controlled(&self.mobs[i]) {
            return;
        }
        let mut mob = self.mobs.remove(i);
        let mut random = self.world.random.clone();
        let mut events = Vec::new();
        {
            let world = self.mob_world_base(&[]);
            mob.tick(&world, &mut random, &mut events);
        }
        // The server owns removal; only its packets take the ghast away.
        mob.removed = false;
        self.world.random = random;
        self.mobs.insert(i, mob);
    }
}
