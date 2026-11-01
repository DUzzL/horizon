#!/usr/bin/env python3
"""An arrow that meets a wind charge, on a dedicated server, against vanilla's, seen by a vanilla client.

A vanilla dedicated server and Horizon's (`horizon-client --dedicated --nogui`) each serve a copy of one flat world
vanilla generated first, with Alice an operator in survival. A still wind charge is summoned above her and
LocatorBarClient (a player made of vanilla's protocol code) fires a loaded crossbow straight up
(ServerboundUseItemPacket) into it: the charge is `#redirectable_projectile`, so `Projectile.isPickable` and the arrow
can hit it, and `Projectile.onHit` turns it along her look (`onRedirectProjectile`, AIM_DEFLECT) and gives it
acceleration power 0.1 (`AbstractHurtingProjectile.onDeflection`), which `applyInertia` adds each tick: a second later
it is 35 to 47 blocks up, past where a speed of 1 would take it.

Only whether the charge is in that band is compared (vanilla's answer is the oracle).
"""
import sys

try:
    from . import test_locator_bar_parity as locator
except ImportError:
    import test_locator_bar_parity as locator

NAME = 'arrow-wind-charge-parity'
# server_dir and generate_world write these server.properties.
locator.PROPERTIES = dict(locator.PROPERTIES, gamemode='survival')
SCRIPT = [
    ('setup', ['chat on',
               'cmd Alice give @s minecraft:crossbow[minecraft:charged_projectiles=[{id:"minecraft:arrow",count:1}]]',
               'cmd Alice execute at @s run summon minecraft:wind_charge ~ ~4.5 ~']),
    ('shoot', ['use Alice -90']),
    ('asked', ['cmd Alice execute if entity @e[type=minecraft:wind_charge,x=-8,y=-30,z=-8,dx=16,dy=40,dz=16]']),
]


def shown(lines):
    """The answer about the charge."""
    out = []
    for line in lines:
        parts = line.split(' ', 3)
        if len(parts) >= 3 and parts[1] == 'step=asked' and parts[2] == 'chat':
            out.append(f'{parts[1]} {parts[3]}')
    return out


def check(vanilla, vanilla_all):
    if not any('commands.execute.conditional.pass' in line or 'passed' in line for line in vanilla):
        return 'the wind charge was not where a turned, accelerating one flies'
    return None


def main():
    lines = locator.one_player(NAME, SCRIPT, shown, check, 'A crossbow arrow fired straight up into a still wind '
                               'charge on a dedicated server: the charge turns along the shooter\'s look and '
                               'speeds up.', step_seconds=1.0)
    print(f'arrow wind charge: {len(lines)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
