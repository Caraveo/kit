# Cinematics release notes

## 0.1.0

Needs Kit 0.0.6 or newer (its players include the Cinematics runtime).

- Cinematic (director) and Cinematic Shot components.
- Cuts and blends between shots, including blending in from the game's camera.
- Shot cameras fly along paths sketched in Canvas mode.
- Look at an object, zoom, shake, captions.
- Letterbox, fade in/out, skip key, loop.
- Play on start, from x (`Cinematic.Play`, `Stop`, `Skip`, `Playing`, `Current`, `Shot`, `Time`) or from diagrams (custom event `PlayCinematic` with the name; diagrams hear `CinematicStarted` / `CinematicFinished`).
