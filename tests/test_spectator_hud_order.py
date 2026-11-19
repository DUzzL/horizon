"""Regression check for the draw order in vanilla's Hud.extractHotbarAndDecorations."""

from pathlib import Path
import re
import unittest


CLIENT = Path(__file__).resolve().parents[1] / "crates/horizon-client/src"


class SpectatorHudOrderTest(unittest.TestCase):
    def test_spectator_action_follows_locator_bar(self):
        hud = (CLIENT / "game.rs").read_text(encoding="utf-8")
        spectator = (CLIENT / "game/interface/spectator.rs").read_text(encoding="utf-8")
        action_method = re.search(r"fn (\w+)\([^\n]+\) \{(?:(?!\n    \}).)*?\.action\(",
                                  spectator, re.DOTALL)
        self.assertIsNotNone(action_method)
        action_call = hud.index(f"g.{action_method[1]}(")
        self.assertLess(hud.index("g.locator_bar_draws("), action_call)
        self.assertLess(action_call, hud.index('self.gui_layer("minecraft:sleep_overlay"'))
