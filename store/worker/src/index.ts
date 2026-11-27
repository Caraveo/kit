interface Env {
  DB: D1Database;
  STORE_BUCKET: R2Bucket;
  REX_API: string;
  ADMIN_USER_IDS: string;
  UPLOAD_SECRET: string;
}

type User = { id: string; email: string; name?: string | null };
type Auth = { user: User; admin: boolean };
const MAX_RELEASE_BYTES = 100 * 1024 * 1024 * 1024;
const MULTIPART_SIZE = 64 * 1024 * 1024;

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", "cache-control": "no-store" } });

const id = () => crypto.randomUUID();
const encode = (value: ArrayBuffer | string) => {
  const bytes = typeof value === "string" ? new TextEncoder().encode(value) : new Uint8Array(value);
  return btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
};

async function uploadToken(artifactId: string, ownerId: string, secret: string, expires = Math.floor(Date.now() / 1000) + 900) {
  const payload = `${artifactId}.${ownerId}.${expires}`;
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return `${encode(payload)}.${encode(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(payload)))}`;
}

async function validUploadToken(token: string | null, artifactId: string, ownerId: string, secret: string) {
  if (!token) return false;
  const [encoded, signature] = token.split(".");
  if (!encoded || !signature) return false;
  const payload = atob(encoded.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - encoded.length % 4) % 4));
  const [tokenArtifact, tokenOwner, expiry] = payload.split(".");
  if (tokenArtifact !== artifactId || tokenOwner !== ownerId || Number(expiry) < Math.floor(Date.now() / 1000)) return false;
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["verify"]);
  const bytes = Uint8Array.from(atob(signature.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - signature.length % 4) % 4)), (char) => char.charCodeAt(0));
  return crypto.subtle.verify("HMAC", key, bytes, new TextEncoder().encode(payload));
}

async function authenticate(request: Request, env: Env): Promise<Auth | null> {
  const token = request.headers.get("authorization")?.match(/^Bearer\s+(.+)$/i)?.[1];
  if (!token) return null;
  const response = await fetch(`${env.REX_API.replace(/\/$/, "")}/me`, { headers: { authorization: `Bearer ${token}` } });
  if (!response.ok) return null;
  const body = (await response.json()) as { user?: User };
  if (!body.user?.id || !body.user.email) return null;
  const admins = new Set((env.ADMIN_USER_IDS || "").split(",").map((value) => value.trim()).filter(Boolean));
  return { user: body.user, admin: admins.has(body.user.id) };
}

async function rexJson(path: string, init: RequestInit, env: Env) {
  const response = await fetch(`${env.REX_API.replace(/\/$/, "")}${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...(init.headers || {}) },
  });
  return new Response(await response.text(), {
    status: response.status,
    headers: { "content-type": response.headers.get("content-type") || "application/json" },
  });
}

function requireAuth(auth: Auth | null) {
  return auth ? null : json({ error: "unauthorized" }, 401);
}

function requireAdmin(auth: Auth | null) {
  if (!auth) return json({ error: "unauthorized" }, 401);
  return auth.admin ? null : json({ error: "forbidden" }, 403);
}

function safeSlug(value: unknown) {
  return typeof value === "string" && /^[a-z0-9][a-z0-9-]{1,63}$/.test(value);
}

async function catalog(env: Env) {
  const { results } = await env.DB.prepare(`
    SELECT l.id, l.slug, l.title, l.description, l.cover_key, l.screenshots_json,
           r.id AS revision_id, r.metadata_json, a.filename, a.size_bytes, a.sha256
    FROM listings l
    JOIN revisions r ON r.id = l.approved_revision_id AND r.status = 'approved'
    JOIN artifacts a ON a.revision_id = r.id
    WHERE l.status = 'published'
    ORDER BY l.title
  `).all();
  return results.map((row: Record<string, unknown>) => ({
    id: row.id,
    slug: row.slug,
    title: row.title,
    description: row.description,
    cover: row.cover_key,
    screenshots: JSON.parse(String(row.screenshots_json || "[]")),
    release: {
      revisionId: row.revision_id,
      version: (JSON.parse(String(row.metadata_json || "{}")) as { version?: string }).version || "0.0.0",
      filename: row.filename,
      sizeBytes: row.size_bytes,
      sha256: row.sha256,
      downloadUrl: `/api/v1/download/${row.revision_id}`,
    },
  }));
}

async function download(revisionId: string, env: Env) {
  const row = await env.DB.prepare(`
    SELECT a.object_key, a.filename, a.size_bytes, a.sha256
    FROM artifacts a JOIN revisions r ON r.id = a.revision_id
    JOIN listings l ON l.approved_revision_id = r.id
    WHERE a.revision_id = ? AND r.status = 'approved' AND l.status = 'published'
  `).bind(revisionId).first<Record<string, unknown>>();
  if (!row) return json({ error: "not_found" }, 404);
  const object = await env.STORE_BUCKET.get(String(row.object_key));
  if (!object) return json({ error: "not_found" }, 404);
  return new Response(object.body, {
    headers: {
      "content-type": "application/gzip",
      "content-disposition": `attachment; filename="${String(row.filename).replace(/["\r\n]/g, "")}"`,
      "x-kit-sha256": String(row.sha256),
      "content-length": String(row.size_bytes),
      "cache-control": "private, max-age=60",
    },
  });
}

function dashboard(kind: "developer" | "admin") {
  const title = kind === "admin" ? "Kit Store Review" : "Kit Store Developer";
  const endpoint = kind === "admin" ? "/api/v1/admin/reviews" : "/api/v1/developer/listings";
  return new Response(`<!doctype html>
<meta charset="utf-8"><title>${title}</title>
<style>body{font:16px system-ui;max-width:980px;margin:40px auto;padding:0 20px;background:#10131a;color:#e8edf5}input,button,textarea{font:inherit;padding:8px;margin:4px;background:#1b2230;color:inherit;border:1px solid #39465d;border-radius:6px}button{cursor:pointer}pre{white-space:pre-wrap;background:#171c26;padding:16px;border-radius:8px}</style>
<h1>${title}</h1>
<p>Uses the same publish.kitgameengine.win API as Kit Store and Kit Editor.</p>
<button onclick="signIn()">Sign in with Kit</button> <button onclick="load()">Refresh</button>
<pre id="out">Sign in with your Kit account to load this dashboard.</pre>
<script>
const out=document.querySelector('#out');
let token=localStorage.getItem('kit_rex_token')||'';
async function signIn(){
 const r=await fetch('/api/v1/auth/device/start',{method:'POST'});
 const d=await r.json();
 if(!r.ok){out.textContent=JSON.stringify(d,null,2);return}
 out.textContent='Approve Kit sign-in in the browser window that opens, then wait here...';
 window.open(d.verification_uri_complete||d.verification_uri,'_blank','noopener');
 for(let i=0;i<60;i++){
   await new Promise(resolve=>setTimeout(resolve,Math.max(1000,(d.interval||5)*1000)));
   const p=await fetch('/api/v1/auth/device/token',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({device_code:d.device_code})});
   const v=await p.json();
   if(v.access_token){token=v.access_token;localStorage.setItem('kit_rex_token',token);return load()}
   if(v.error&&v.error!=='authorization_pending'&&v.error!=='slow_down'){out.textContent=JSON.stringify(v,null,2);return}
 }
 out.textContent='Sign-in timed out. Please try again.';
}
async function load(){
 if(!token){out.textContent='Click Sign in with Kit first.';return}
 const r=await fetch('${endpoint}',{headers:{authorization:'Bearer '+token}});
 out.textContent=await r.text();
}
if(token) load();
</script>`, { headers: { "content-type": "text/html; charset=utf-8" } });
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (request.method === "OPTIONS") return new Response(null, { headers: { "access-control-allow-origin": "*", "access-control-allow-headers": "authorization,content-type", "access-control-allow-methods": "GET,POST,PUT,OPTIONS" } });
    try {
      if (request.method === "GET" && url.pathname === "/developer") return dashboard("developer");
      if (request.method === "GET" && url.pathname === "/admin") return dashboard("admin");
      if (url.pathname === "/api/v1/auth/device/start" && request.method === "POST") {
        return rexJson("/device/start", {
          method: "POST",
          body: JSON.stringify({ client_name: "Kit Store" }),
        }, env);
      }
      if (url.pathname === "/api/v1/auth/device/token" && request.method === "POST") {
        const body = (await request.json()) as { device_code?: string };
        if (!body.device_code) return json({ error: "device_code_required" }, 400);
        return rexJson("/device/token", {
          method: "POST",
          body: JSON.stringify({ device_code: body.device_code }),
        }, env);
      }
      if (request.method === "GET" && url.pathname === "/api/v1/catalog") return json({ games: await catalog(env) });
      if (request.method === "GET" && url.pathname.startsWith("/api/v1/games/")) {
        const slug = url.pathname.split("/").pop()!;
        const games = await catalog(env);
        const game = games.find((entry) => entry.slug === slug);
        return game ? json(game) : json({ error: "not_found" }, 404);
      }
      if (request.method === "GET" && url.pathname.startsWith("/api/v1/download/")) return download(url.pathname.split("/").pop()!, env);

      const auth = await authenticate(request, env);
      if (url.pathname === "/api/v1/developer/listings" && request.method === "GET") {
        const denied = requireAuth(auth); if (denied) return denied;
        const { results } = await env.DB.prepare("SELECT * FROM listings WHERE owner_id = ? ORDER BY updated_at DESC").bind(auth!.user.id).all();
        return json({ listings: results });
      }
      if (url.pathname === "/api/v1/developer/listings" && request.method === "POST") {
        const denied = requireAuth(auth); if (denied) return denied;
        const body = (await request.json()) as { slug?: string; title?: string; description?: string };
        if (!safeSlug(body.slug) || typeof body.title !== "string" || !body.title.trim()) return json({ error: "invalid_listing" }, 400);
        const listingId = id();
        await env.DB.prepare("INSERT INTO listings (id, owner_id, slug, title, description) VALUES (?, ?, ?, ?, ?)").bind(listingId, auth!.user.id, body.slug, body.title.trim(), body.description || "").run();
        return json({ id: listingId }, 201);
      }
      const listingMatch = url.pathname.match(/^\/api\/v1\/developer\/listings\/([^/]+)\/revisions$/);
      if (listingMatch && request.method === "POST") {
        const denied = requireAuth(auth); if (denied) return denied;
        const listing = await env.DB.prepare("SELECT * FROM listings WHERE id = ? AND owner_id = ?").bind(listingMatch[1], auth!.user.id).first();
        if (!listing) return json({ error: "not_found" }, 404);
        const body = (await request.json()) as { version?: string; notes?: string; filename?: string; sizeBytes?: number; sha256?: string };
        if (!body.version || !body.filename || !body.sha256 || !Number.isSafeInteger(body.sizeBytes) || body.sizeBytes! <= 0 || body.sizeBytes! > MAX_RELEASE_BYTES) {
          return json({ error: "invalid_revision", maxBytes: MAX_RELEASE_BYTES }, 400);
        }
        const revisionId = id();
        const artifactId = id();
        const objectKey = `pending/${auth!.user.id}/${revisionId}/${body.filename.replace(/[^A-Za-z0-9._-]/g, "_")}`;
        await env.DB.batch([
          env.DB.prepare("INSERT INTO revisions (id, listing_id, owner_id, metadata_json) VALUES (?, ?, ?, ?)").bind(revisionId, listingMatch[1], auth!.user.id, JSON.stringify({ version: body.version, notes: body.notes || "" })),
          env.DB.prepare("INSERT INTO artifacts (id, revision_id, object_key, filename, size_bytes, sha256, content_type) VALUES (?, ?, ?, ?, ?, ?, ?)").bind(artifactId, revisionId, objectKey, body.filename, body.sizeBytes, body.sha256.toLowerCase(), "application/gzip"),
        ]);
        if (!env.UPLOAD_SECRET) return json({ error: "upload_not_configured" }, 503);
        const multipart = body.sizeBytes > 5 * 1024 * 1024 * 1024
          ? await env.STORE_BUCKET.createMultipartUpload(objectKey)
          : null;
        return json({
          revisionId,
          artifactId,
          maxBytes: MAX_RELEASE_BYTES,
          uploadUrl: `/api/v1/developer/artifacts/${artifactId}`,
          uploadToken: await uploadToken(artifactId, auth!.user.id, env.UPLOAD_SECRET),
          ...(multipart ? { multipart: { uploadId: multipart.uploadId, partSize: MULTIPART_SIZE, partUrl: `/api/v1/developer/artifacts/${artifactId}/parts`, completeUrl: `/api/v1/developer/artifacts/${artifactId}/complete` } } : {}),
        }, 201);
      }
      const artifactMatch = url.pathname.match(/^\/api\/v1\/developer\/artifacts\/([^/]+)$/);
      if (artifactMatch && request.method === "PUT") {
        const denied = requireAuth(auth); if (denied) return denied;
        if (!env.UPLOAD_SECRET || !(await validUploadToken(request.headers.get("x-kit-upload-token"), artifactMatch[1], auth!.user.id, env.UPLOAD_SECRET))) return json({ error: "invalid_upload_token" }, 403);
        const row = await env.DB.prepare("SELECT a.object_key, a.size_bytes FROM artifacts a JOIN revisions r ON r.id = a.revision_id WHERE a.id = ? AND r.owner_id = ? AND r.status = 'pending'").bind(artifactMatch[1], auth!.user.id).first<{ object_key: string; size_bytes: number }>();
        if (!row) return json({ error: "not_found" }, 404);
        const length = Number(request.headers.get("content-length") || 0);
        if (!length || length !== row.size_bytes || length > MAX_RELEASE_BYTES || length > 5 * 1024 * 1024 * 1024) return json({ error: "invalid_upload_size", maxBytes: MAX_RELEASE_BYTES }, 413);
        await env.STORE_BUCKET.put(row.object_key, request.body, { httpMetadata: { contentType: "application/gzip" } });
        return json({ uploaded: true });
      }
      const partMatch = url.pathname.match(/^\/api\/v1\/developer\/artifacts\/([^/]+)\/parts$/);
      if (partMatch && request.method === "PUT") {
        const denied = requireAuth(auth); if (denied) return denied;
        if (!env.UPLOAD_SECRET || !(await validUploadToken(request.headers.get("x-kit-upload-token"), partMatch[1], auth!.user.id, env.UPLOAD_SECRET))) return json({ error: "invalid_upload_token" }, 403);
        const uploadId = url.searchParams.get("uploadId");
        const partNumber = Number(url.searchParams.get("partNumber"));
        if (!uploadId || !Number.isInteger(partNumber) || partNumber < 1 || partNumber > 10000) return json({ error: "invalid_part" }, 400);
        const length = Number(request.headers.get("content-length") || 0);
        if (!length || length > 5 * 1024 * 1024 * 1024) return json({ error: "invalid_part_size" }, 413);
        const row = await env.DB.prepare("SELECT a.object_key FROM artifacts a JOIN revisions r ON r.id = a.revision_id WHERE a.id = ? AND r.owner_id = ? AND r.status = 'pending'").bind(partMatch[1], auth!.user.id).first<{ object_key: string }>();
        if (!row) return json({ error: "not_found" }, 404);
        const multipart = env.STORE_BUCKET.resumeMultipartUpload(row.object_key, uploadId);
        const part = await multipart.uploadPart(partNumber, request.body);
        return json({ partNumber, etag: part.etag });
      }
      const completeMatch = url.pathname.match(/^\/api\/v1\/developer\/artifacts\/([^/]+)\/complete$/);
      if (completeMatch && request.method === "POST") {
        const denied = requireAuth(auth); if (denied) return denied;
        if (!env.UPLOAD_SECRET || !(await validUploadToken(request.headers.get("x-kit-upload-token"), completeMatch[1], auth!.user.id, env.UPLOAD_SECRET))) return json({ error: "invalid_upload_token" }, 403);
        const body = (await request.json()) as { uploadId?: string; parts?: Array<{ partNumber: number; etag: string }> };
        if (!body.uploadId || !body.parts?.length || body.parts.length > 10000) return json({ error: "invalid_parts" }, 400);
        const row = await env.DB.prepare("SELECT a.object_key, a.size_bytes FROM artifacts a JOIN revisions r ON r.id = a.revision_id WHERE a.id = ? AND r.owner_id = ? AND r.status = 'pending'").bind(completeMatch[1], auth!.user.id).first<{ object_key: string; size_bytes: number }>();
        if (!row) return json({ error: "not_found" }, 404);
        const object = await env.STORE_BUCKET.resumeMultipartUpload(row.object_key, body.uploadId).complete(body.parts);
        if (object.size !== row.size_bytes || object.size > MAX_RELEASE_BYTES) {
          await env.STORE_BUCKET.delete(row.object_key);
          return json({ error: "completed_size_mismatch", expectedBytes: row.size_bytes, maxBytes: MAX_RELEASE_BYTES }, 409);
        }
        return json({ uploaded: true, sizeBytes: object.size });
      }
      if (url.pathname === "/api/v1/admin/reviews" && request.method === "GET") {
        const denied = requireAdmin(auth); if (denied) return denied;
        const { results } = await env.DB.prepare("SELECT r.*, l.slug, l.title FROM revisions r JOIN listings l ON l.id = r.listing_id WHERE r.status = 'pending' ORDER BY r.submitted_at").all();
        return json({ revisions: results });
      }
      const reviewMatch = url.pathname.match(/^\/api\/v1\/admin\/reviews\/([^/]+)$/);
      if (reviewMatch && request.method === "POST") {
        const denied = requireAdmin(auth); if (denied) return denied;
        const body = (await request.json()) as { decision?: "approved" | "changes"; notes?: string };
        if (body.decision !== "approved" && body.decision !== "changes") return json({ error: "invalid_decision" }, 400);
        if (body.decision === "changes" && !body.notes?.trim()) return json({ error: "notes_required" }, 400);
        const revision = await env.DB.prepare("SELECT * FROM revisions WHERE id = ? AND status = 'pending'").bind(reviewMatch[1]).first<{ listing_id: string }>();
        if (!revision) return json({ error: "not_found" }, 404);
        const artifact = await env.DB.prepare("SELECT object_key FROM artifacts WHERE revision_id = ?").bind(reviewMatch[1]).first<{ object_key: string }>();
        if (!artifact || !(await env.STORE_BUCKET.head(artifact.object_key))) return json({ error: "artifact_not_uploaded" }, 409);
        const status = body.decision === "approved" ? "approved" : "changes_requested";
        await env.DB.batch([
          env.DB.prepare("INSERT INTO reviews (id, revision_id, reviewer_id, decision, notes) VALUES (?, ?, ?, ?, ?)").bind(id(), reviewMatch[1], auth!.user.id, body.decision, body.notes || ""),
          env.DB.prepare("UPDATE revisions SET status = ? WHERE id = ?").bind(status, reviewMatch[1]),
          ...(body.decision === "approved" ? [env.DB.prepare("UPDATE listings SET status = 'published', approved_revision_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?").bind(reviewMatch[1], revision.listing_id)] : []),
        ]);
        return json({ status });
      }
      return json({ error: "not_found" }, 404);
    } catch (error) {
      console.error(error);
      return json({ error: "internal_error" }, 500);
    }
  },
};
