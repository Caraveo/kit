---
title: Diagrams (visual logic)
description: Make objects react to events with nodes and wires, no code.
---

A **Diagram** is visual logic for an object: events on the left, wired to actions on the right. Diagrams run directly in the engine. No script is generated or needed.

## Open the Diagram view

Select an object and press <kbd>7</kbd> (or **◫ Diagram** in the viewport bar). The view opens under the viewport.

If the object has no Diagram yet, click **+ Add Diagram**. You can also use **Add component → Diagram** in the inspector. Kit creates `diagrams/<object>.kdiagram` with an **On Start** node.

## Building a graph

- **Add a node:** right-click the canvas, or click **+ Node**. Type to search, then press <kbd>Enter</kbd> or click a node.
- **Wire it:** drag from a pin to another pin of the same kind and type.
  - White arrow pins carry *execution*: what happens next.
  - Colored round pins carry *values*.
  - Drop a wire on empty canvas to add a node that fits it, already connected.
- **Delete a wire:** click it to select it (it lights up), then press <kbd>Del</kbd> or <kbd>Backspace</kbd>. Right-clicking a pin removes all of its wires.
- **Values:** an input with no wire has an inline value (number, color, checkbox…) right on the node.
- **Move around:**
  - Drag a node by its title. Shift + click selects several.
  - Drag the background to pan, and scroll to zoom.
  - **Frame all** brings everything back into view.
- **Edit:** with the mouse over the view, <kbd>Del</kbd> deletes, <kbd>Ctrl</kbd> + <kbd>D</kbd> duplicates, and <kbd>Ctrl</kbd> + <kbd>Z</kbd> / <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Z</kbd> undo and redo.

Changes save to the `.kdiagram` file immediately.

## Node settings and variables

Select a node to edit its settings in the strip under the canvas. Settings include:

- The event name.
- Which dataset row a data node reads.
- Compare and Math operators.
- World or local space.
- The **Target**: *Self* (the object with the Diagram), or any object picked from the scene.

With no node selected, the strip shows the graph's **Variables**. Each variable is either *per object* or *shared in the scene*.

## Script fields

Scripts can share fields with diagrams:

- `PUBLIC` fields appear as **Get** nodes.
- `EDIT` fields appear as **Get** and **Set** nodes.

They're listed under **SCRIPT FIELDS** in the node picker. See [Classes](/scripting/classes/).

## Problems and live view

Problems are counted at the top of the view. A node with an error has a red outline, and hovering it explains why. Examples are a wire between different types, or a dataset field that was deleted. A graph with errors doesn't run.

While playing, the view is live:

- Nodes and wires light up as they run.
- Hovering a pin shows its last value.
- A node that failed is outlined in red.

Edits made while playing apply the next time you press Play.
