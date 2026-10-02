"""Refresh the website's game list from the checked-in catalog."""

import csv
import json
import re
from pathlib import Path


WEB = Path(__file__).resolve().parent
CATALOG = WEB.parents[1] / "catalog" / "catalog.json"
GROUPS = [
    "Alex' favourites",
    "Roguelikes",
    "Stories",
    "Survival",
    "Campaigns",
    "Achievements",
]
GROUP_TAGLINES = {
    "Roguelikes": "Keep a rare build alive",
    "Stories": "See where a different choice leads",
    "Survival": "Give a beloved character one more chance",
    "Campaigns": "Keep a month-long campaign going",
    "Achievements": "Finish the challenge you nearly beat",
}
FAVOURITES = {
    "Hades",
    "Noita",
    "FTL: Faster Than Light",
    "The Banner Saga",
    "Europa Universalis IV",
    "Six Ages: Ride Like the Wind",
    "The Life and Suffering of Sir Brante",
    "This War of Mine",
}
CATEGORY_GROUPS = {
    "Permadeath roguelike/roguelite runs": "Roguelikes",
    "Run-based games needing serialization validation": "Roguelikes",
    "Committed narrative/autosave choices": "Stories",
    "Persistent consequences ordinary reload cannot fully undo": "Stories",
    "Ironman / single-save campaigns": "Campaigns",
    "Limited/coarse rollback systems": "Campaigns",
    "Legacy/succession death systems": "Survival",
    "Hardcore one-life characters or worlds": "Survival",
    "Permanent roster or asset loss": "Survival",
    "Restricted or resource-gated saving": "Survival",
}
GROUP_OVERRIDES = {
    "Outward": "Survival",
    "Six Ages 2: Lights Going Out": "Stories",
}
CONTEXT_OVERRIDES = {
    "Hades": (
        "An escape attempt can come together around rare boons, a favorite "
        "weapon aspect, and the right keepsake. A single boss fight can end "
        "the build you spent the run assembling."
    ),
    "Hades II": (
        "Melinoë's journey through the Underworld or across the Surface can "
        "come together around rare boons, a weapon aspect, and the right keepsake. "
        "A single guardian fight can end the build you spent the run assembling."
    ),
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
    "Balatro": "cards",
    "Baldur's Gate 3": "dice-d20",
    "Barony": "chess-knight",
    "Battle Brothers": "shield-halved",
    "BattleTech": "gear",
    "Brotato": "gun",
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
    "Hades": "skull",
    "Hades II": "skull",
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
    "Monster Train 2": "train",
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
    "Rogue Legacy 2": "chess-knight",
    "Roboquest": "robot",
    "ScourgeBringer": "swords",
    "Shiren the Wanderer: The Tower of Fortune and the Dice of Fate": "map",
    "Signs of the Sojourner": "compass",
    "Six Ages 2: Lights Going Out": "cow",
    "Six Ages: Ride Like the Wind": "cow",
    "Slasher's Keep": "dungeon",
    "Slay the Spire": "trophy",
    "Slay the Spire 2": "trophy",
    "Spelunky 2": "mountain",
    "Star of Providence": "star-shooting",
    "Star Trek: Infinite": "shuttle-space",
    "State of Decay 2": "biohazard",
    "Stellaris": "satellite",
    "Streets of Rogue": "mask",
    "Sunless Sea": "anchor",
    "Sunless Skies": "cloud-moon",
    "Suzerain": "landmark",
    "Synthetik: Legion Rising": "robot",
    "Tales of Maj'Eyal": "hat-wizard",
    "Teleglitch: Die More Edition": "gun",
    "Terraria": "heart-pulse",
    "The Banner Saga": "flag-swallowtail",
    "The Binding of Isaac: Rebirth": "skull",
    "The Council": "mask",
    "The Life and Suffering of Sir Brante": "book-open",
    "The Long Dark": "snowflake",
    "This War of Mine": "person-rifle",
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
    "XCOM 2": "alien",
    "XCOM: Enemy Unknown": "alien",
    "Yes, Your Grace": "crown",
}
WEBSITES = {
    "dungeon-crawl-stone-soup": "https://crawl.develz.org/",
    "nethack": "https://www.nethack.org/",
}
ART_OVERRIDES = {
    "dungeon-crawl-stone-soup": {
        "hero": "games/dungeon-crawl-stone-soup-hero.png",
        "logo": None,
        "artStyle": "self-contained",
    },
    "nethack": {
        "hero": "games/nethack-hero.png",
        "logo": "games/nethack-logo.gif",
        "artStyle": "nethack",
    },
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
with (WEB.parents[1] / "catalog" / "games.csv").open(encoding="utf-8-sig", newline="") as source:
    rows = list(csv.DictReader(source))
    categories_by_name = {
        row["Name"]: row["Category"]
        for row in rows
        if row["Product fit"] == "Keep"
    }
    achievement_detours = {
        row["Name"]
        for row in rows
        if row["Product fit"] == "Keep"
        and re.search(r"\bachievements?\b", row["Info"] + " " + row["Actionable feedback"], re.I)
    }
    achievement_detours.add("Baldur's Gate 3")  # Foehammer is named without the word "achievement".
if set(ICON_BY_NAME) != {game["name"] for game in catalog}:
    raise ValueError("Icon mapping does not match the catalog")
if {game["id"] for game in catalog if not game["id"].startswith("steam-")} != set(WEBSITES):
    raise ValueError("Website links do not match games without Steam IDs")
if set(categories_by_name) != {game["name"] for game in catalog}:
    raise ValueError("CSV categories do not match the catalog")
if set(categories_by_name.values()) != set(CATEGORY_GROUPS):
    raise ValueError("Website groups do not match the CSV categories")
if (set(CATEGORY_GROUPS.values()) != set(GROUPS[1:-1])
        or set(GROUP_TAGLINES) != set(GROUPS[1:])
        or not (FAVOURITES | achievement_detours | GROUP_OVERRIDES.keys()) <= set(categories_by_name)
        or not set(GROUP_OVERRIDES.values()) <= set(GROUPS[1:-1])):
    raise ValueError("Website group assignments are incomplete")
for icon in set(ICON_BY_NAME.values()):
    if not (WEB / "dist" / "icons" / "value" / f"{icon}.svg").is_file():
        raise ValueError(f"Missing Font Awesome icon: {icon}")
for art in ART_OVERRIDES.values():
    for path in (art["hero"], art["logo"]):
        if path and not (WEB / "dist" / path).is_file():
            raise ValueError(f"Missing game artwork: {path}")

ordered = sorted(catalog, key=lambda game: game["name"].casefold())
data = [
    {
        "name": game["name"],
        "group": "Alex' favourites" if game["name"] in FAVOURITES
                 else "Achievements" if game["name"] in achievement_detours
                 else GROUP_OVERRIDES.get(game["name"], CATEGORY_GROUPS[categories_by_name[game["name"]]]),
        "context": CONTEXT_OVERRIDES.get(game["name"]) or " ".join(
            [section_text(game["info"], "Context"), section_text(game["info"], "How progress is saved")]
        ),
        "value": section_text(game["info"], "SaveScummer value"),
        "icon": ICON_BY_NAME[game["name"]],
        "steam": int(game["id"][6:]) if game["id"].startswith("steam-") else None,
        "website": WEBSITES.get(game["id"]),
        "hero": image_path(game["id"], "hero.jpg"),
        "logo": image_path(game["id"], "logo.png"),
        **ART_OVERRIDES.get(game["id"], {}),
    }
    for game in ordered
]
(WEB / "dist" / "games-data.js").write_text(
    "// Generated by apps/web/update-games.py from catalog/catalog.json and catalog/games.csv.\n"
    + "window.savescummerGameGroups = "
    + json.dumps([
        {
            "title": title,
            "tagline": GROUP_TAGLINES.get(title),
            "games": [
                {key: value for key, value in game.items() if key != "group"}
                for game in data if game["group"] == title
            ],
        }
        for title in GROUPS
    ], ensure_ascii=False, indent=2)
    + ";\n",
    encoding="utf-8",
)
print(f"Updated {len(data)} games in {len(GROUPS)} groups.")
