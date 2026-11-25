# Kit Circuits

Circuits are installable extensions that connect Kit to services and
technologies: multiplayer, storage, analytics, commerce and more. They are
**not** engine modules. A project opts in per Circuit, pins exact versions,
and can update, downgrade, disable or remove them without touching anything
else.

Anyone can publish one: Kit, companies, or the community.

```
Kit editor ── Project → Circuit Manager ──► Circuit API (circuit.kitgameengine.win/api/v1)
                 Available · Installed · Updates      ├─ registry: circuits, releases, publishers
                 versions · install · remove          └─ downloads → GitHub release assets
                                                          (checksums and signatures verified in Kit)
REX account (login.kitgameengine.win/account) ── Circuits → hosted services (e.g. Multiplayer)
```

Circuit Manager only manages extensions. Hosted services (servers, usage,
keys) live on the account website or in a Circuit's own editor interfaces.

## Layout of a Circuit

```
my-circuit/
  circuit.toml        manifest (required)
  NOTES.md            release notes (optional, named in the manifest)
  scripts/…           x scripts the game loads (entry.runtime / entry.server)
  interfaces/…        .xui editor interfaces (entry.editor)
  docs/, README.md    anything else ships as-is
```

## circuit.toml

```toml
schema = 1

[circuit]
id = "acme.leaderboards"          # <publisher>.<name>, lowercase
name = "Leaderboards"
publisher = "acme"                 # must match the id's prefix and the registry publisher
description = "Global and friends leaderboards."
version = "1.2.0"                  # semantic versioning
channel = "stable"                 # stable | beta | alpha
license = "MIT"                    # SPDX
kit = ">=0.0.4, <0.2.0"            # Kit versions it works with
platforms = ["linux", "macos", "windows", "web"]
architectures = ["any"]            # "any" = no native code
permissions = ["network.client"]   # shown before install
documentation = "https://…"
repository = "https://…"
release_notes = "NOTES.md"

[dependencies]                     # other Circuits: id = semver requirement
"kit.multiplayer" = "^0.1"

[entry]
runtime = ["scripts/Leaderboards.x"]
editor = ["interfaces/leaderboards.xui"]

[config.board_size]                # optional per-project settings
type = "integer"
default = 10
description = "Entries shown"
```

Permissions: `network.client`, `network.server`, `project.read`,
`project.write`, `editor.ui`, `native`.

## Packages and releases

`cargo run -p kit_circuit --bin kit-circuit -- pack path/to/circuit` writes:

- `<id>-<version>.kitcircuit`: a gzip'd tar. It is deterministic, so the same
  source always gives the same bytes. It holds only plain files, with no
  symlinks and no paths that leave the folder.
- `circuit.json`: the manifest, the package's SHA-256 and size, the release
  notes and, when signed, an Ed25519 signature over
  `kit-circuit/v1\n<id>\n<version>\n<sha256>\n`.

Publish both as assets of a GitHub release tagged `<tag_prefix><version>`. The
registry picks new releases up hourly, or on `POST /circuits/<id>/sync`. Once a
version is registered it never changes. To fix something, publish a new
version.

Signing: set `KIT_CIRCUIT_SIGNING_KEY` (base64 32-byte Ed25519 secret) and
`KIT_CIRCUIT_KEY_ID` from your CI's secret store. Never put the key in the repo.
Register the public key with the registry. Once a publisher has keys, Kit
refuses that publisher's unsigned releases.

## Installing (what Kit does)

1. Download the package (the Circuit API checks the developer's sign-in, then
   points to the file; the token is never sent to the file host).
2. Verify the size, the SHA-256 and the publisher signature when present. Any
   mismatch stops the install.
3. Check compatibility: Kit version, platform, dependencies, and that no
   installed Circuit needs a different version of this one.
4. Unpack into `circuits/.staging/` and check that its `circuit.toml` matches
   the release.
5. Swap it into `circuits/<id>/`, then write `circuits.lock` atomically. If
   anything fails, put the previous version back.

Installing never touches your scenes, scripts or assets. Kit refuses to
replace a `circuits/<id>` folder it didn't install. Uninstalling removes only
`circuits/<id>/` and its lockfile entry. Data a Circuit added to scenes (e.g.
Multiplayer's per-entity `"realtime"` settings) stays and is ignored, so
reinstalling brings it back.

`circuits.lock` records the id, version, SHA-256, source and enabled switch of
every Circuit. Commit it with the project so every machine and every export
uses the same bytes. Each project keeps its own versions; nothing is updated
automatically.

## How Circuits extend Kit (and why)

Kit is Rust, and exported games are a prebuilt `kit-player` plus the project's
files. Players have no Rust toolchain, and the web, iOS and consoles can't load
native libraries at runtime. So:

- **Content Circuits** (now): x scripts, `.xui` interfaces, data and assets.
  They run in Kit's script sandbox on every platform. Discovery and installation
  never run any of a Circuit's code. Scripts only run when the game runs.
- **Engine-backed Circuits** (Kit's own, like Multiplayer): the native part
  (e.g. the Realtime client) ships with Kit's players and does nothing unless
  the project has the Circuit installed and enabled. That keeps one prebuilt
  player per platform while still letting projects opt out.
- **Native Circuits** (reserved): `entry.native` maps `<os>-<arch>` to a shared
  library with a C ABI. This Kit verifies but **never loads** them. Loading
  native code would give a Circuit full access to the player's machine, so it
  will require signatures, the `native` permission, an explicit prompt, and
  desktop only.

## Security notes

- Packages are verified before unpacking, and unpacking writes only plain
  files inside the target.
- Publishing is tied to the publisher's account. Signatures bind a package to
  its id and version, so a valid package can't be replayed under another name.
- Circuits never receive the developer's REX token, Kit's infrastructure credentials or
  GitHub credentials. Hosted services use their own scoped keys (see
  Multiplayer's backend keys).
- Community scripts run with the game's script permissions. Review what a
  Circuit asks for before installing it.
