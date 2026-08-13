#!/usr/bin/env python3
"""
Validate skill names against known harness built-in commands (#105).

Fails on exact collision, warns on prefix near-miss.

Usage:
    python3 tools/validate_skill_names.py [--check]

With --check: exit 1 on collision, 0 otherwise.
Without --check: print report, always exit 0.
"""

import sys
import os
import json
from pathlib import Path
from typing import Set, Tuple

REPO_ROOT = Path(__file__).parent.parent
COMMANDS_DIR = REPO_ROOT / "commands"
BUILTINS_DIR = REPO_ROOT / "tools"

# Near-misses: warn but don't fail
NEAR_MISSES = {
    "release": "release-notes",
    "onboard": "team-onboarding",
    "migrate": "migrate-to-skills",
}


def load_builtins(harness: str) -> Set[str]:
    """Load built-in command names for a harness."""
    path = BUILTINS_DIR / f"builtins-{harness}.txt"
    if not path.exists():
        # Harnesses now come from harness_matrix.json, so a new target arrives
        # here the moment it is registered. Returning an empty set would compare
        # our thirteen names against nothing and report "no collisions" for that
        # harness forever — the check would silently opt out of every target
        # added after this one.
        raise SystemExit(
            f"error: no built-in list for harness {harness!r}\n"
            f"  create {path.relative_to(REPO_ROOT).as_posix()} (one command name per line, "
            "`#` for comments) listing that harness's built-in commands, or remove the harness "
            "from tools/harness_matrix.json"
        )
    
    names = set()
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            # Strip leading slash if present
            name = line.lstrip("/")
            names.add(name)
    return names


def get_skill_names() -> Set[str]:
    """Get all skill names from commands/ directory."""
    if not COMMANDS_DIR.exists():
        return set()
    return {f.stem for f in COMMANDS_DIR.glob("*.md")}


def check_collision(skill: str, builtins: Set[str], harness: str) -> Tuple[bool, str]:
    """Check if skill collides with a built-in. Returns (is_collision, message)."""
    if skill in builtins:
        return True, f"COLLISION: /{skill} shadows {harness} built-in /{skill}"
    
    if skill in NEAR_MISSES:
        near = NEAR_MISSES[skill]
        if near in builtins:
            return False, f"NEAR-MISS: /{skill} close to {harness} built-in /{near}"
    
    return False, ""


def main():
    check_mode = "--check" in sys.argv
    
    skills = get_skill_names()
    if not skills:
        print("No commands found in commands/")
        sys.exit(0)
    
    # The harness names live under the "harnesses" key. Reading the top level
    # yields ["version", "harnesses", "features"], none of which is a harness,
    # so every lookup missed and the gate reported "no collisions" having
    # compared our command names against zero builtins.
    with open(REPO_ROOT / "tools" / "harness_matrix.json", encoding="utf-8") as f:
        harnesses = list(json.load(f)["harnesses"].keys())
    collisions = []
    warnings = []
    
    for harness in harnesses:
        builtins = load_builtins(harness)
        if not builtins:
            continue
        
        for skill in sorted(skills):
            is_collision, msg = check_collision(skill, builtins, harness)
            if is_collision:
                collisions.append(msg)
            elif msg:
                warnings.append(msg)
    
    if collisions or warnings:
        print("Skill name validation report:")
        print()
        
        if collisions:
            print("COLLISIONS (must fix):")
            for msg in collisions:
                print(f"  ✗ {msg}")
            print()
        
        if warnings:
            print("NEAR-MISSES (consider renaming):")
            for msg in warnings:
                print(f"  ⚠ {msg}")
            print()
    
    if check_mode and collisions:
        print(f"FAILED: {len(collisions)} collision(s) found")
        print()
        print("Fix: rename the skill to avoid shadowing a harness built-in.")
        print("See CONTRIBUTING.md for the add-a-skill checklist.")
        sys.exit(1)
    
    if not collisions and not warnings:
        print("✓ No skill name collisions or near-misses")
    
    sys.exit(0)


if __name__ == "__main__":
    main()
