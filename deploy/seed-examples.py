#!/usr/bin/env python3
"""Run as root on the orch host: python3 seed-examples.py.

Adds six fictional catalog examples, not playable builds. Back up the database
first with `orch db backup foyer db`. Re-running never overwrites existing rows.
Credentials are read in memory from the running API, never printed or saved.
"""

import datetime
import json
import subprocess
import urllib.request
import uuid


def execute(*args):
    return subprocess.check_output(["orch", "exec", "foyer", *args])


def identifier(name):
    return str(uuid.uuid5(uuid.NAMESPACE_URL, "https://foyer.lol/examples/" + name))


def main():
    environment = dict(
        item.split("=", 1)
        for item in execute("api", "--", "cat", "/proc/1/environ").decode().split("\0")
        if "=" in item
    )
    status = json.loads(subprocess.check_output(["orch", "app", "status", "foyer", "--json"]))
    address = next(item["address"] for item in status["allocations"]
                   if item["service"] == "db" and item.get("address"))
    now = datetime.datetime.now(datetime.timezone.utc).isoformat()
    owner = identifier("owner")
    developer = identifier("developer")
    statements = []

    def insert(table, columns, values):
        statements.append({
            "sql": f"INSERT OR IGNORE INTO {table} ({','.join(columns)}) "
                   f"VALUES ({','.join('?' for _ in values)})",
            "args": [{"type": "text", "value": value} for value in values],
        })

    # An intentionally invalid password hash disables password sign-in.
    insert("users", ["id", "username", "email", "password_hash", "created_at"],
           [owner, "foyer-examples", "examples@foyer.invalid", "!disabled", now])
    insert("developers", ["id", "owner_id", "slug", "display_name", "bio",
                          "created_at", "updated_at"],
           [developer, owner, "foyer-examples", "Foyer Examples",
            "Fictional games demonstrating the Foyer catalog. These are not playable releases.",
            now, now])

    games = [
        ("moss-and-lanterns", "Moss & Lanterns", "Relight a forest one shrine at a time.",
         "Explore an overgrown woodland, redirect beams of light and restore its forgotten shrines. "
         "A gentle concept with no combat or timers.", ["exploration", "puzzle", "cozy"]),
        ("brine", "Brine", "A salvage puzzler at the bottom of the sea.",
         "Guide a tiny submersible through sunken wrecks. Solve rooms built around pressure, "
         "buoyancy and the cable keeping you connected to the surface.", ["puzzle", "atmospheric"]),
        ("last-train-to-umber", "Last Train to Umber", "Keep the night train running.",
         "Manage coal, water and a carriage of unusual passengers as your train crosses "
         "a continent slowly going dark.", ["strategy", "management", "roguelite"]),
        ("kelp-garden", "Kelp Garden", "Grow a reef, feed a village.",
         "Tend underwater gardens, restore a coastal ecosystem and get to know a small "
         "seaside community, one harvest at a time.", ["cozy", "farming"]),
        ("signal-fire", "Signal Fire", "Hold the beacon through the night.",
         "Defend a hilltop signal against waves of shadows. Choose upgrades between "
         "short rounds and balance your defenses against dwindling fuel.",
         ["action", "roguelite", "shooter"]),
        ("paper-orbit", "Paper Orbit", "Fold paper, launch satellites.",
         "Turn flat sheets into clever spacecraft. Experiment with folds and momentum "
         "to solve compact orbital physics puzzles.", ["puzzle", "physics"]),
    ]
    for slug, title, tagline, description, tags in games:
        insert("games", ["id", "developer_id", "slug", "title", "tagline", "description",
                         "tags", "status", "published_at", "created_at", "updated_at"],
               [identifier(slug), developer, "example-" + slug, title + " (Example)",
                tagline, "FICTIONAL EXAMPLE — This listing demonstrates Foyer. "
                "No playable build is available.\n\n" + description,
                json.dumps(tags), "published", now, now, now])

    # Conditional steps keep inserts atomic: a failure skips commit and rolls back.
    steps = [{"stmt": {"sql": "BEGIN IMMEDIATE"}}]
    for statement in statements + [{"sql": "COMMIT"}]:
        steps.append({"condition": {"type": "ok", "step": len(steps) - 1},
                      "stmt": statement})
    steps.append({"condition": {"type": "not", "cond": {"type": "ok", "step": len(steps) - 1}},
                  "stmt": {"sql": "ROLLBACK"}})
    payload = {"requests": [{"type": "batch", "batch": {"steps": steps}}, {"type": "close"}]}
    request = urllib.request.Request(
        f"http://{address}:8080/v2/pipeline", json.dumps(payload).encode(),
        headers={"Authorization": "Bearer " + environment["FOYER_DATABASE_AUTH_TOKEN"],
                 "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        result = json.load(response)["results"][0]
    if result["type"] == "error":
        raise RuntimeError(result["error"])
    errors = [error for error in result["response"]["result"]["step_errors"] if error]
    if errors:
        raise RuntimeError(errors)
    print("Six example listings are present under /games/example-*. No builds or login credentials created.")


if __name__ == "__main__":
    main()
