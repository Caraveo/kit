# Kit website

The static site lives in `dist/` and has no build step. The editor gallery uses Kit screenshots. The asset gallery uses rendered previews from FORGE.

## Preview locally

```sh
python3 -m http.server 4178 --directory site/dist
```

## Deploy kitgameengine.win

From the repository root, with Wrangler signed in to the Cloudflare account that owns the domain:

```sh
npx wrangler deploy --config site/wrangler.jsonc
```

The homepage links to the Kit AI Worker for pricing and account management.

## Deploy to the existing Cloudflare Pages project

Run from the repository root with Wrangler signed in to the Cloudflare account that owns the `kit-game-engine` Pages project.

```sh
npx wrangler pages deploy site/dist --project-name kit-game-engine --branch main
```
