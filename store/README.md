# Kit Store

The first Store milestone uses `https://publish.kitgameengine.win` for the
catalog, developer publishing, review, and approved artifact downloads.

`worker/` contains the Cloudflare Worker, D1 migration, and private R2 binding.
It deliberately does not expose draft or pending objects. The API validates REX
developer tokens through the existing REX `/me` endpoint and checks admin IDs
server-side for every review action.

Before deploying, replace `database_id` in `worker/wrangler.jsonc`, create the
private R2 bucket, set `ADMIN_USER_IDS` and the `UPLOAD_SECRET` Worker secret,
and apply the migration:

```sh
cd store/worker
npm install
npx wrangler d1 migrations apply kit-store --remote
npx wrangler secret put UPLOAD_SECRET
npx wrangler deploy
```

The remaining DNS and Cloudflare account setup are intentionally deployment
configuration, not checked-in credentials.

The Linux launcher is built with:

```sh
cargo build --release -p kit-store
install -Dm755 target/release/kit-store ~/.local/bin/kit-store
install -Dm644 packaging/linux/io.github.caraveo.KitStore.desktop \
  ~/.local/share/applications/io.github.caraveo.KitStore.desktop
```
