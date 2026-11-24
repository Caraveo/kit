# Hooks

**Hooks** are Kit's export platform modules: one folder per platform. Each folder's `hook.toml` tells the editor's **Export** wizard what the platform is, whether Kit can build it, and how.

```
hooks/
├─ linux/    windows/    mac/      ← desktop: Kit exports these itself
├─ web/                            ← WebGPU web builds: Kit exports it
├─ steam/                          ← Request access (Steamworks)
├─ android/  ios/                  ← planned
└─ xbox/  ps5/  ps4/  switch/      ← Request access (licensed console SDKs)
```

## Access levels

| `access` | Meaning | In Export |
|---|---|---|
| `open` | Kit builds it: Linux, Windows, Mac, Web. | **Ready** (or a short note if a player file is missing) |
| `request` | The platform holder licenses the tools, and you apply to them for access. | **Request access**: a button to their developer portal, plus the requirements |
| `planned` | Kit doesn't support it yet. | **Coming later**, with what's missing |

## `hook.toml`

```toml
id = "switch"                 # folder name
name = "Nintendo Switch"
icon = "🎮"
group = "console"             # desktop | web | mobile | console
access = "request"            # open | request | planned
summary = "One sentence shown in Export."
request_url = "https://developer.nintendo.com/"   # where to apply (request/planned)
request_label = "Apply on the Nintendo Developer Portal"
requirements = ["Approval from Nintendo", "…"]

# Only in installed (licensed) hooks:
[export]
command = "export"            # a program in the hook's folder
args = ["--project", "{project}", "--out", "{out}", "--title", "{title}", "--build-id", "{build_id}"]
```

The editor ships with the manifests in this folder built in.

## Installed hooks

A hook installed next to the editor replaces the built-in one with the same `id`. Kit looks in these places:

- `hooks/<id>/` beside the `kit-editor` program;
- `Kit.app/Contents/Resources/hooks/<id>/` on macOS;
- `/usr/share/kit/hooks/<id>/` for Linux packages.

This is how a **Request access** platform becomes exportable. Once you're approved, the platform's licensed Kit hook goes into, for example, `hooks/switch/`. Its manifest has an `[export]` section, so the card turns **Ready**. When you export, Kit runs the command with:

| Placeholder | Value |
|---|---|
| `{project}` | the project folder |
| `{out}` | the output folder chosen in Export |
| `{title}` | the game's title |
| `{build_id}` | the editor's build ID (a hook's player must match it) |
| `{kit_dir}` | the folder containing the editor |

The command must exit with status 0 and leave the game in `{out}`. Its last line of output, or of its errors if it fails, is shown to the user.

Kit never runs hooks from inside a project folder. Only hooks installed next to the editor run, so opening a project from someone else can't run code on your computer.
