"""A rocket boosting a server client's own player, run through the live method."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("rustc"), "requires Rust")
class RemoteOwnRocketTests(unittest.TestCase):
    def test_the_spark_follows_the_moved_player_at_the_rocket_hand(self):
        source = (ROOT / "crates/horizon-client/src/game/remote_projectiles.rs").read_text()
        start = source.index("    fn remote_player_id(")
        end = source.index("    /// One tick of interpolation", start)
        fixture = r'''
#![allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Vec3 { x: f64, y: f64, z: f64 }
impl Vec3 {
    fn new(x: f64, y: f64, z: f64) -> Self { Self { x, y, z } }
    fn add_vector(self, o: Vec3) -> Self { Self::new(self.x + o.x, self.y + o.y, self.z + o.z) }
}
fn view_vector(_yaw: f32, _pitch: f32) -> [f64; 3] { [0.0, 0.6, 0.8] }
struct Remote { entity_id: i32 }
struct Clock { runs: bool }
impl Clock { fn world_runs(&self) -> bool { self.runs } }
struct Player { position: Vec3, velocity: Vec3, fall_flying: bool }
#[derive(Default)]
struct Rocket { attached_to: Option<u32>, position: Vec3, velocity: Vec3 }
struct Game {
    remote: Option<Remote>, clock: Clock, player: Option<Player>,
    rockets: Vec<Rocket>, yaw: f32, pitch: f32, sparks: Vec<[f64; 3]>,
}
impl Game {
fn rocket_hand_angle(&self) -> [f64; 3] { [0.5, 0.0, 0.0] }
fn rocket_spark(&mut self, r: &Rocket) { self.sparks.push([r.position.x, r.position.y, r.position.z]); }
METHOD
}
fn game(runs: bool) -> Game {
    Game {
        remote: Some(Remote { entity_id: 7 }),
        clock: Clock { runs },
        // Where the player's own tick has just moved it.
        player: Some(Player {
            position: Vec3::new(10.0, 70.0, 20.0),
            velocity: Vec3::new(0.0, -0.2, 1.0),
            fall_flying: true,
        }),
        rockets: vec![
            Rocket { attached_to: Some(7), ..Rocket::default() },
            Rocket { attached_to: Some(8), position: Vec3::new(1.0, 2.0, 3.0), ..Rocket::default() },
            Rocket { attached_to: None, position: Vec3::new(4.0, 5.0, 6.0), ..Rocket::default() },
        ],
        yaw: 0.0, pitch: 0.0, sparks: Vec::new(),
    }
}
fn main() {
    let mut g = game(true);
    g.tick_own_rockets();
    // FireworkRocketEntity.tick: movement + look * 0.1 + (look * 1.5 - movement) * 0.5.
    let boosted = Vec3::new(
        0.0 + 0.0 * 0.1 + (0.0 * 1.5 - 0.0) * 0.5,
        -0.2 + 0.6 * 0.1 + (0.6 * 1.5 - -0.2) * 0.5,
        1.0 + 0.8 * 0.1 + (0.8 * 1.5 - 1.0) * 0.5,
    );
    let p = g.player.as_ref().unwrap();
    assert_eq!(p.velocity, boosted);
    assert_eq!(p.position, Vec3::new(10.0, 70.0, 20.0));
    // The rocket at the moved player, half a block out at the rocket hand.
    assert_eq!(g.rockets[0].position, Vec3::new(10.5, 70.0, 20.0));
    assert_eq!(g.rockets[0].velocity, boosted);
    assert_eq!(g.sparks, vec![[10.5, 70.0, 20.0]]);
    // Rockets of another entity, or free ones, tick with the other entities.
    assert_eq!(g.rockets[1].position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(g.rockets[2].position, Vec3::new(4.0, 5.0, 6.0));

    // Not gliding: it follows the player without boosting it.
    let mut g = game(true);
    g.player.as_mut().unwrap().fall_flying = false;
    g.tick_own_rockets();
    assert_eq!(g.player.as_ref().unwrap().velocity, Vec3::new(0.0, -0.2, 1.0));
    assert_eq!(g.sparks.len(), 1);

    // A frozen level ticks no entities.
    let mut g = game(false);
    g.tick_own_rockets();
    assert!(g.sparks.is_empty());
    assert_eq!(g.player.as_ref().unwrap().velocity, Vec3::new(0.0, -0.2, 1.0));
}
'''.replace("METHOD", source[start:end].replace("pub(super) ", ""))
        with tempfile.TemporaryDirectory(prefix="remote-own-rockets-") as directory:
            path = Path(directory)
            (path / "main.rs").write_text(fixture)
            built = subprocess.run(
                ["rustc", "--edition=2024", str(path / "main.rs"), "-o", str(path / "probe")],
                capture_output=True,
                text=True,
            )
            self.assertEqual(built.returncode, 0, built.stderr)
            result = subprocess.run([str(path / "probe")], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
