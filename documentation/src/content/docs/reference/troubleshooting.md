---
title: Troubleshooting
description: Fixes for common problems.
---

## Kit says "Vulkan was not found" or "needs Vulkan 1.3"
Update your GPU driver. On Linux install Mesa's Vulkan drivers (`mesa-vulkan-drivers`) or NVIDIA's driver. Check support with `vulkaninfo --summary`.

## The editor doesn't respond to typing in the game
Click the viewport while playing; its border lights up when the game has the keyboard. <kbd>Esc</kbd> gives the keyboard back to the editor.

## My script doesn't do anything
- Check the console for red errors. After an error, a script stops until you fix and save it.
- Make sure the object's **Script → File** points to your script.
- Methods must be spelled exactly: `Init`, `Update(FLOAT dt)`.
- Using a module? Add it to `TAKE` at the top.

## "`x` is not declared"
x needs a type the first time: `FLOAT x: 0.0`. After that, assign with `x: 5`.

## Objects fall through the floor
The floor needs a **Collider**. Very fast objects may need **Fast (CCD)** in their Physics section.

## Collision events don't fire
Both objects need colliders. For `OnTrigger`, one of them must have **Trigger** checked.

## I changed something while playing and it disappeared
Play mode always restores the scene when you stop. Make lasting changes in edit mode.

## Logs
Run the editor or player from a terminal to see detailed logs. Set `RUST_LOG=debug` for more, and `KIT_VALIDATION=1` to check for GPU errors.
