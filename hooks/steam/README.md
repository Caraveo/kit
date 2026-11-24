# Steam hook

Steam games are Kit's Windows, Mac and Linux builds, uploaded with Steamworks (SteamPipe).

This platform is **Request access**: its tools are licensed by the platform holder, so Kit can't include them. In **Export**, the card's **Request access** button opens the application page.

Once you're approved, install the licensed Kit hook for this platform into `hooks/steam/` next to the Kit editor. Its `hook.toml` adds an `[export]` command, and the card turns **Ready**.

## Requirements

- A Steamworks partner account (one-time app fee per game)
- Your Windows, Mac or Linux export from Kit
- SteamPipe (Steamworks SDK) to upload builds
