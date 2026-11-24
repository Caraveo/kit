---
title: World physics settings
description: Gravity, simulation rate, and turning simulation on or off.
---

Open **Project → Project settings…** (or **World settings…** in any Physics section).

| Setting | |
|---|---|
| **Simulate rigid bodies** | Off: nothing falls or gets pushed, but colliders still detect touches and triggers. |
| **Gravity** | X, Y, Z in m/s². Presets: **Earth** (−9.81), **Moon** (−1.62), **Zero-G**. For a top-down game, use Zero-G. |
| **Step rate** | Simulation steps per second (30–240). Higher is more accurate and costs more CPU. 60 is a good default. |

Click **Save**: the settings are written to `kit.project` and **apply immediately**, even while the game is playing.

Scripts can change gravity at runtime with `Physics.SetGravity(0, -20, 0)`.

In `kit.project` the settings look like this:

```toml
[physics]
enabled = true
gravity = [0.0, -9.81, 0.0]
rate = 60
```
