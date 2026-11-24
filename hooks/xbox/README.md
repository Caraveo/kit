# Xbox hook

Xbox builds need Microsoft's licensed GDK and an approved developer account.

This platform is **Request access**: its tools are licensed by the platform holder, so Kit can't include them. In **Export**, the card's **Request access** button opens the application page.

Once you're approved, install the licensed Kit hook for this platform into `hooks/xbox/` next to the Kit editor. Its `hook.toml` adds an `[export]` command, and the card turns **Ready**.

## Requirements

- Approval from Microsoft (ID@Xbox or a publisher)
- The Microsoft GDK and dev kit hardware
- The Kit Xbox hook, installed into hooks/xbox
