---
title: File formats
description: kit.project, .kscene and .x files.
---

## `.kscene`: scenes

JSON. A list of entities. Each entity has an `id`, a `name`, an optional `parent` id, a `transform`, and one optional field per component:

```json
{
  "version": 1,
  "entities": [
    {
      "id": 0,
      "name": "Player",
      "transform": { "translation": [0, 0.5, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] },
      "model": { "mesh": "Cube", "color": [0.76, 0.66, 1.0, 1.0] },
      "script": { "path": "scripts/player.x" },
      "collider": { "shape": { "kind": "Box", "size": [1, 1, 1] } },
      "rigid_body": { "kind": "Dynamic", "mass": 1.0 },
      "path": { "points": [[0, 0.5, 0], [4, 0.5, 0]], "speed": 3.0, "mode": "Loop" }
    }
  ]
}
```

- Rotations are quaternions `[x, y, z, w]`.
- Colors are sRGB `[r, g, b, a]` from 0 to 1.
- Missing fields use defaults, so hand-written scenes can be short.
- **Fields Kit doesn't recognize are kept** when the scene is saved. Files from newer Kit versions or plugins don't lose data in older editors.

## `.x`: scripts

UTF-8 text. See [x basics](/scripting/basics/).

## `kit.project`

TOML. See [Project settings](/projects/project-settings/).
