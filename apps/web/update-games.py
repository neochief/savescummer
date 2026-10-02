"""Refresh the website's game list from the checked-in catalog."""

import json
import re
from pathlib import Path


WEB = Path(__file__).resolve().parent
CATALOG = WEB.parents[1] / "catalog" / "catalog.json"
FEATURED = [
    "steam-1086940", "steam-250900", "steam-646570", "steam-108600",
    "steam-105600", "steam-268500", "steam-588650", "steam-632360",
    "steam-212680", "steam-281990", "steam-394360", "steam-219740",
    "steam-311690", "steam-881100", "steam-379430", "steam-203770",
    "steam-590380", "steam-594570", "steam-418530", "steam-333640",
]
CONTEXT_OVERRIDES = {
    "Terraria": (
        "Terraria is a sandbox adventure where your character and world develop "
        "separately. It normally saves progress, but Hardcore character death is "
        "permanent. That makes it relevant for players attempting the Extra Life "
        "achievement, and a useful checkpoint needs both the character and world saves."
    ),
    "FTL: Faster Than Light": (
        "FTL is a spaceship strategy game where a run builds up crew, weapons, "
        "and systems over several sectors. Its save lets you continue an active "
        "run, but that save is overwritten as you advance and disappears when the "
        "run ends. It offers no normal way back to an earlier version of your ship."
    ),
    "Noita": (
        "Noita is a roguelike where a long run can accumulate custom wands, rare "
        "spells, perks, and changes to the world. Save & Quit lets you continue "
        "that run, but it does not give you earlier states to return to. A "
        "checkpoint must capture the complete world and run save."
    ),
    "Crusader Kings II": (
        "Crusader Kings II is a strategy game about guiding a dynasty through "
        "succession, politics, and war. It has normal saves, but achievement-compatible "
        "Ironman campaigns continually overwrite one save. A bad succession or "
        "war can therefore undo decades of progress with no manual rollback point."
    ),
}
ICON_BY_NAME = {
    "ADOM: Ancient Domains of Mystery": "dungeon",
    "Baldur's Gate 3": "dice-d20",
    "Barony": "chess-knight",
    "Battle Brothers": "shield-halved",
    "BattleTech": "gear",
    "Caveblazers": "mountain",
    "Caves of Qud": "flask",
    "Cogmind": "gears",
    "Crusader Kings II": "chess-rook",
    "Crypt of the NecroDancer": "music",
    "Darkwood": "tree",
    "Dead Cells": "skull-crossbones",
    "Dome Keeper": "shield-halved",
    "Don't Starve": "fire",
    "Dungeon Crawl Stone Soup": "dungeon",
    "Enter the Gungeon": "bomb",
    "Europa Universalis IV": "coins",
    "Europa Universalis V": "landmark",
    "FTL: Faster Than Light": "shuttle-space",
    "Gears Tactics": "gear",
    "Grim Dawn": "ghost",
    "Hearts of Iron IV": "flag",
    "Heaven's Vault": "scroll",
    "HighFleet": "ship",
    "Imperator: Rome": "landmark",
    "Into the Breach": "robot",
    "Invisible, Inc.": "mask",
    "Jagged Alliance 3": "person-rifle",
    "Jupiter Hell": "rocket",
    "Kingdom Come: Deliverance": "chess-knight",
    "Monster Train": "train",
    "My Summer Car": "car",
    "NEO Scavenger": "compass",
    "NetHack": "skull",
    "Noita": "hat-wizard",
    "Nuclear Throne": "radiation",
    "One Step from Eden": "wand-magic-sparkles",
    "OTXO": "gun",
    "Outward": "person-hiking",
    "Pathologic 2": "biohazard",
    "Pentiment": "book-open",
    "Project Zomboid": "biohazard",
    "Revita": "heart-pulse",
    "Risk of Rain": "meteor",
    "Risk of Rain 2": "meteor",
    "Risk of Rain Returns": "meteor",
    "Road 96": "road",
    "Roboquest": "robot",
    "ScourgeBringer": "bolt",
    "Shiren the Wanderer: The Tower of Fortune and the Dice of Fate": "map",
    "Signs of the Sojourner": "compass",
    "Six Ages 2: Lights Going Out": "scroll",
    "Six Ages: Ride Like the Wind": "horse",
    "Slasher's Keep": "dungeon",
    "Slay the Spire": "trophy",
    "Spelunky 2": "mountain",
    "Star of Providence": "star",
    "Star Trek: Infinite": "shuttle-space",
    "State of Decay 2": "biohazard",
    "Stellaris": "satellite",
    "Streets of Rogue": "mask",
    "Sunless Sea": "anchor",
    "Sunless Skies": "cloud",
    "Suzerain": "landmark",
    "Synthetik: Legion Rising": "robot",
    "Tales of Maj'Eyal": "hat-wizard",
    "Teleglitch: Die More Edition": "gun",
    "Terraria": "heart",
    "The Banner Saga": "flag",
    "The Binding of Isaac: Rebirth": "skull",
    "The Council": "mask",
    "The Life and Suffering of Sir Brante": "book-open",
    "The Long Dark": "snowflake",
    "This War of Mine": "heart",
    "Torchlight II": "fire",
    "Total War: Attila": "chess-rook",
    "Total War: Rome II": "landmark",
    "Total War: Shogun 2": "torii-gate",
    "Total War: Three Kingdoms": "dragon",
    "Total War: Warhammer II": "dragon",
    "Vampyr": "skull-crossbones",
    "Void War": "rocket",
    "Voidigo": "gem",
    "Warhammer 40,000: Chaos Gate - Daemonhunters": "skull-crossbones",
    "Wildermyth": "book-open",
    "Wizard of Legend": "wand-magic-sparkles",
    "XCOM 2": "crosshairs",
    "XCOM: Enemy Unknown": "crosshairs",
    "Yes, Your Grace": "crown",
}


def section_text(info, section):
    match = re.search(rf"\*\*{re.escape(section)}:\*\*\s*(.*?)(?:\n\n|$)", info, re.S)
    if not match:
        raise ValueError(f"Missing {section}")
    text = re.sub(r"\*\*(.*?)\*\*", r"\1", match.group(1))
    return " ".join(text.split())


def image_path(game_id, suffix):
    if not game_id.startswith("steam-"):
        return None
    path = f"games/{game_id[6:]}-{suffix}"
    return path if (WEB / "dist" / path).is_file() else None


catalog = json.loads(CATALOG.read_text(encoding="utf-8"))["games"]
by_id = {game["id"]: game for game in catalog}
if len(by_id) != len(catalog) or any(game_id not in by_id for game_id in FEATURED):
    raise ValueError("Featured game IDs do not match the catalog")
if set(ICON_BY_NAME) != {game["name"] for game in catalog}:
    raise ValueError("Icon mapping does not match the catalog")
for icon in set(ICON_BY_NAME.values()):
    if not (WEB / "dist" / "icons" / "value" / f"{icon}.svg").is_file():
        raise ValueError(f"Missing Font Awesome icon: {icon}")

ordered = [by_id[game_id] for game_id in FEATURED]
ordered += sorted(
    (game for game in catalog if game["id"] not in FEATURED),
    key=lambda game: game["name"].casefold(),
)
data = [
    {
        "name": game["name"],
        "context": CONTEXT_OVERRIDES.get(game["name"]) or " ".join(
            [section_text(game["info"], "Context"), section_text(game["info"], "How progress is saved")]
        ),
        "value": section_text(game["info"], "SaveScummer value"),
        "icon": ICON_BY_NAME[game["name"]],
        "hero": image_path(game["id"], "hero.jpg"),
        "logo": image_path(game["id"], "logo.png"),
    }
    for game in ordered
]
(WEB / "dist" / "games-data.js").write_text(
    "// Generated by apps/web/update-games.py from catalog/catalog.json.\n"
    + "window.savescummerGames = "
    + json.dumps(data, ensure_ascii=False, indent=2)
    + ";\n",
    encoding="utf-8",
)
print(f"Updated {len(data)} games ({len(FEATURED)} featured).")
