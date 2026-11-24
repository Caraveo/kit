// Kit web player: runs a Kit game (WebAssembly) and draws it with WebGPU.
//
// kit_web.wasm runs the whole game and, every frame, writes a command buffer
// into its memory: meshes and textures to upload, then the frame's draws
// (see web/player/src/lib.rs, module `op`). This file only uploads and draws.

const OP = { GEOMETRY: 1, FREE_GEOMETRY: 2, TEXTURE: 3, FRAME: 5, DRAW: 6, END: 7, POST: 8 };
const HDR = "rgba16float";
const PASS = { OPAQUE: 0, BLENDED: 1, SPRITE: 2, TERRAIN: 3 };
const DRAW_STRIDE = 256; // per-draw uniform slot (WebGPU's minimum dynamic offset alignment)
const DRAW_BYTES = 128; // Push in shaders.wgsl: mat4 + 4 vec4
const FRAME_BYTES = 128; // Frame in shaders.wgsl
const SAMPLES = 4;

const params = new URLSearchParams(location.search);
// Protected (signed) exports pin the publisher's key and mode here. The game
// then loads only game.kitseal and checks its signature before reading it.
const KIT_TRUST = null;
const ui = {
  canvas: document.getElementById("kit-canvas"),
  status: document.getElementById("kit-status"),
  bar: document.getElementById("kit-bar"),
  loading: document.getElementById("kit-loading"),
  error: document.getElementById("kit-error"),
};

function fail(title, detail, help) {
  ui.loading.hidden = true;
  ui.error.hidden = false;
  ui.error.querySelector("h1").textContent = title;
  ui.error.querySelector("p").textContent = detail;
  const box = document.getElementById("kit-help");
  box.replaceChildren();
  if (help) box.append(help);
  window.kitError = `${title}: ${detail}`;
  console.error(title, detail);
}

// ------------------------------------------------- turning WebGPU on

/** The browser and system, for WebGPU instructions. */
function browserInfo() {
  const ua = navigator.userAgent;
  const os = /Android/.test(ua)
    ? "android"
    : /iPhone|iPad|iPod/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)
      ? "ios"
      : /Mac OS X|Macintosh/.test(ua)
        ? "mac"
        : /Windows/.test(ua)
          ? "windows"
          : "linux";
  const name = /Edg\//.test(ua)
    ? "edge"
    : /OPR\/|Opera/.test(ua)
      ? "opera"
      : /Firefox\//.test(ua)
        ? "firefox"
        : /Chrome\/|Chromium\//.test(ua)
          ? "chrome"
          : /Safari\//.test(ua)
            ? "safari"
            : "other";
  const version = Number((ua.match(/(?:Firefox|Chrome|Version)\/(\d+)/) || [])[1] || 0);
  return { os, name, version };
}

/** A step: text with `code` parts and an optional Copy button for an address. */
function step(parts) {
  const li = document.createElement("li");
  for (const part of parts) {
    if (typeof part === "string") {
      li.append(part);
    } else if (part.code) {
      const code = document.createElement("code");
      code.textContent = part.code;
      li.append(code);
      if (part.copy) {
        const button = document.createElement("button");
        button.className = "kit-button";
        button.textContent = "Copy";
        button.onclick = async () => {
          try {
            await navigator.clipboard.writeText(part.code);
            button.textContent = "Copied ✓";
          } catch {
            // Clipboard blocked: select the text so Ctrl+C works.
            const range = document.createRange();
            range.selectNodeContents(code);
            getSelection().removeAllRanges();
            getSelection().addRange(range);
            button.textContent = "Press Ctrl+C";
          }
        };
        li.append(button);
      }
    }
  }
  return li;
}

/**
 * How to turn WebGPU on in this browser. `blocked` = the browser has WebGPU
 * but refused this computer's GPU (blocklist or no hardware acceleration).
 */
function webgpuHelp(blocked) {
  const { os, name, version } = browserInfo();
  const box = document.createElement("div");
  const intro = document.createElement("p");
  const list = document.createElement("ol");
  const steps = [];
  const chromeLike = { chrome: "chrome", edge: "edge", opera: "opera" }[name];
  if (name === "firefox") {
    intro.textContent = blocked
      ? "Firefox has WebGPU but turned it off for this graphics card. You can turn it on:"
      : "Turn on WebGPU in Firefox:";
    steps.push(step(["Open a new tab and go to ", { code: "about:config", copy: true }, " (accept the warning)."]));
    if (!blocked || version < 141) steps.push(step(["Search for ", { code: "dom.webgpu.enabled" }, " and set it to ", { code: "true" }, "."]));
    steps.push(step(["Search for ", { code: "gfx.webgpu.ignore-blocklist" }, " and set it to ", { code: "true" }, "."]));
    steps.push(step(["Come back here and press ", { code: "Try again" }, "."]));
  } else if (chromeLike) {
    const flags = `${chromeLike}://flags`;
    intro.textContent =
      os === "linux" || blocked
        ? `${name[0].toUpperCase() + name.slice(1)} keeps WebGPU off on ${os === "linux" ? "Linux" : "this graphics card"} for now. You can turn it on:`
        : "Your browser has WebGPU turned off. You can turn it on:";
    steps.push(step(["Open a new tab and go to ", { code: `${flags}/#enable-unsafe-webgpu`, copy: true }, " and set it to ", { code: "Enabled" }, "."]));
    if (os === "linux") steps.push(step(["Then go to ", { code: `${flags}/#enable-vulkan`, copy: true }, " and set it to ", { code: "Enabled" }, "."]));
    steps.push(step(["Check that ", { code: `${chromeLike}://settings/system`, copy: true }, " has ", { code: "Use graphics acceleration when available" }, " on."]));
    steps.push(step(["Press ", { code: "Relaunch" }, " at the bottom of the flags page, then come back and press ", { code: "Try again" }, "."]));
  } else if (name === "safari") {
    intro.textContent = version >= 26 ? "Safari has WebGPU turned off. Turn it on:" : "WebGPU arrives in Safari 26. On older Safari you can turn it on:";
    if (os === "ios") {
      steps.push(step(["Open ", { code: "Settings → Apps → Safari → Advanced → Feature Flags" }, "."]));
    } else {
      steps.push(step(["In Safari, open ", { code: "Settings → Advanced" }, " and tick ", { code: "Show features for web developers" }, "."]));
      steps.push(step(["Open ", { code: "Settings → Feature Flags" }, "."]));
    }
    steps.push(step(["Turn on ", { code: "WebGPU" }, ", then come back and press ", { code: "Try again" }, "."]));
  } else {
    intro.textContent = "This browser doesn't support WebGPU yet.";
  }
  list.append(...steps);
  const retry = document.createElement("button");
  retry.className = "kit-button primary";
  retry.textContent = "Try again";
  retry.onclick = () => location.reload();
  const other = document.createElement("p");
  other.className = "kit-other";
  other.textContent =
    name === "other" || (name === "safari" && os !== "mac" && os !== "ios")
      ? "Or open this page in a recent Chrome, Edge or Firefox."
      : `Or open this page in a browser with WebGPU on by default: Chrome or Edge on Windows, macOS or Android, or Safari 26 on Mac, iPhone and iPad.`;
  box.append(intro, list);
  if (steps.length) box.append(retry);
  box.append(other);
  return box;
}

async function fetchWithProgress(url, onProgress) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: ${res.status} ${res.statusText}`);
  const total = Number(res.headers.get("content-length")) || 0;
  if (!res.body || !total) return new Uint8Array(await res.arrayBuffer());
  const reader = res.body.getReader();
  const out = new Uint8Array(total);
  let got = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    out.set(value, got);
    got += value.length;
    onProgress(got / total);
  }
  return out;
}

// ------------------------------------------------------------------ wasm

class Game {
  constructor(instance) {
    this.x = instance.exports;
  }
  get bytes() {
    return new Uint8Array(this.x.memory.buffer);
  }
  text() {
    const p = this.x.kit_text_ptr();
    const n = this.x.kit_text_len();
    return new TextDecoder().decode(this.bytes.subarray(p, p + n));
  }
  /** Copy bytes into wasm memory, call f(ptr, len), free them. */
  withBytes(data, f) {
    const p = this.x.kit_alloc(data.length);
    this.bytes.set(data, p);
    try {
      return f(p, data.length);
    } finally {
      this.x.kit_free(p, data.length);
    }
  }
  shader() {
    this.x.kit_shader();
    return this.text();
  }
  postShader() {
    this.x.kit_post_shader();
    return this.text();
  }
  /** Forward Kit's log lines to the browser console. Returns error lines. */
  drainLogs() {
    this.x.kit_logs();
    const errors = [];
    for (const line of this.text().split("\n")) {
      if (!line) continue;
      const level = line[0];
      const msg = line.slice(1);
      if (level === "3") {
        console.error(msg);
        errors.push(msg);
      } else if (level === "2") console.warn(msg);
      else console.log(msg);
    }
    return errors;
  }
}

// --------------------------------------------------------------- WebGPU

class Renderer {
  constructor(device, context, format, shaderCode) {
    this.device = device;
    this.context = context;
    this.format = format;
    this.viewFormat = format.endsWith("-srgb") ? format : `${format}-srgb`;
    this.geometry = new Map();
    this.textures = new Map();
    this.drawCapacity = 0;
    this.size = [0, 0];

    const module = device.createShaderModule({ code: shaderCode, label: "kit builtin" });
    module.getCompilationInfo().then((info) => {
      for (const m of info.messages) {
        (m.type === "error" ? console.error : console.warn)(`shader ${m.lineNum}:${m.linePos} ${m.message}`);
      }
    });

    const VF = GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT;
    this.frameLayout = device.createBindGroupLayout({
      entries: [{ binding: 0, visibility: VF, buffer: { type: "uniform" } }],
    });
    this.textureLayout = device.createBindGroupLayout({
      entries: [
        { binding: 0, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "float" } },
        { binding: 1, visibility: GPUShaderStage.FRAGMENT, sampler: { type: "filtering" } },
      ],
    });
    this.drawLayout = device.createBindGroupLayout({
      entries: [
        {
          binding: 0,
          visibility: VF,
          buffer: { type: "uniform", hasDynamicOffset: true, minBindingSize: DRAW_BYTES },
        },
      ],
    });
    const layout = device.createPipelineLayout({
      bindGroupLayouts: [this.frameLayout, this.textureLayout, this.drawLayout],
    });

    this.frameBuffer = device.createBuffer({ size: FRAME_BYTES, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
    this.frameGroup = device.createBindGroup({
      layout: this.frameLayout,
      entries: [{ binding: 0, resource: { buffer: this.frameBuffer } }],
    });
    this.sampler = device.createSampler({
      magFilter: "linear",
      minFilter: "linear",
      mipmapFilter: "linear",
      addressModeU: "repeat",
      addressModeV: "repeat",
    });
    this.textures.set(0, this.makeTexture(1, 1, [new Uint8Array([255, 255, 255, 255])]));

    // Same pipelines as the desktop renderer (kit_render/src/vulkan.rs).
    const meshBuffers = [
      {
        arrayStride: 32,
        attributes: [
          { shaderLocation: 0, offset: 0, format: "float32x3" },
          { shaderLocation: 1, offset: 12, format: "float32x3" },
          { shaderLocation: 2, offset: 24, format: "float32x2" },
        ],
      },
    ];
    const terrainBuffers = [
      {
        arrayStride: 44,
        attributes: [
          { shaderLocation: 0, offset: 0, format: "float32x3" },
          { shaderLocation: 1, offset: 12, format: "float32x3" },
          { shaderLocation: 2, offset: 24, format: "float32x2" },
          { shaderLocation: 3, offset: 32, format: "float32x3" },
        ],
      },
    ];
    const alpha = {
      color: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha", operation: "add" },
      alpha: { srcFactor: "one", dstFactor: "one-minus-src-alpha", operation: "add" },
    };
    const frontFace = params.get("frontFace") || "ccw";
    const pipeline = (format, vs, fs, buffers, { cull = "none", depthWrite = true, blend } = {}) =>
      device.createRenderPipeline({
        layout,
        vertex: { module, entryPoint: vs, buffers },
        fragment: { module, entryPoint: fs, targets: [{ format, blend }] },
        primitive: { topology: "triangle-list", cullMode: cull, frontFace },
        depthStencil: { format: "depth32float", depthWriteEnabled: depthWrite, depthCompare: "less-equal" },
        multisample: { count: SAMPLES },
      });
    // Scene pipelines per color format: the canvas, or linear HDR when post-processing.
    this.scenePipelines = new Map();
    this.pipelinesFor = (format) => {
      if (!this.scenePipelines.has(format)) {
        this.scenePipelines.set(format, {
          [PASS.OPAQUE]: pipeline(format, "vs_mesh", "fs_mesh", meshBuffers),
          [PASS.BLENDED]: pipeline(format, "vs_mesh", "fs_mesh", meshBuffers, { depthWrite: false, blend: alpha }),
          [PASS.SPRITE]: pipeline(format, "vs_mesh", "fs_sprite", meshBuffers, { depthWrite: false, blend: alpha }),
          [PASS.TERRAIN]: pipeline(format, "vs_terrain", "fs_terrain", terrainBuffers, { cull: "back" }),
        });
      }
      return this.scenePipelines.get(format);
    };
    this.pipelinesFor(this.viewFormat);
    this.post = null;
  }

  /** Post-processing resources (created the first time a volume is active). */
  setupPost(code) {
    const device = this.device;
    const module = device.createShaderModule({ code, label: "kit post" });
    module.getCompilationInfo().then((info) => {
      for (const m of info.messages) {
        (m.type === "error" ? console.error : console.warn)(`post shader ${m.lineNum}:${m.linePos} ${m.message}`);
      }
    });
    const F = GPUShaderStage.FRAGMENT | GPUShaderStage.VERTEX;
    const layout = device.createBindGroupLayout({
      entries: [
        { binding: 0, visibility: F, buffer: { type: "uniform" } },
        { binding: 1, visibility: F, buffer: { type: "uniform", hasDynamicOffset: true, minBindingSize: 256 } },
        { binding: 2, visibility: F, sampler: { type: "filtering" } },
        { binding: 3, visibility: F, texture: { sampleType: "float" } },
        { binding: 4, visibility: F, texture: { sampleType: "float" } },
        // The scene's MSAA depth, read at sample 0 (WebGPU can't resolve depth).
        { binding: 5, visibility: F, texture: { sampleType: "depth", multisampled: true } },
      ],
    });
    const placeholder = device.createTexture({
      size: [1, 1],
      format: HDR,
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.RENDER_ATTACHMENT,
    });
    this.post = {
      module,
      layout,
      pipelineLayout: device.createPipelineLayout({ bindGroupLayouts: [layout] }),
      pipelines: new Map(),
      sampler: device.createSampler({ magFilter: "linear", minFilter: "linear", addressModeU: "clamp-to-edge", addressModeV: "clamp-to-edge" }),
      placeholder: placeholder.createView(),
      uniforms: null,
      passBuffer: null,
      passCapacity: 0,
      targets: null,
    };
  }

  postPipeline(entry, format) {
    const key = `${entry}:${format}`;
    const post = this.post;
    if (!post.pipelines.has(key)) {
      post.pipelines.set(
        key,
        this.device.createRenderPipeline({
          layout: post.pipelineLayout,
          vertex: { module: post.module, entryPoint: "vs_post" },
          fragment: { module: post.module, entryPoint: entry, targets: [{ format }] },
          primitive: { topology: "triangle-list" },
        }),
      );
    }
    return post.pipelines.get(key);
  }

  /** Scene HDR target and temps, reused while the size stays the same. */
  postTargets(width, height, divisors) {
    const t = this.post.targets;
    if (t && t.width === width && t.height === height) return t;
    if (t) {
      t.hdrMsaa.destroy();
      t.hdr.destroy();
      t.temps.forEach((x) => x?.destroy());
    }
    const make = (w, h, samples = 1) =>
      this.device.createTexture({
        size: [Math.max(1, w), Math.max(1, h)],
        sampleCount: samples,
        format: HDR,
        usage: GPUTextureUsage.RENDER_ATTACHMENT | (samples === 1 ? GPUTextureUsage.TEXTURE_BINDING : 0),
      });
    this.post.targets = {
      width,
      height,
      hdrMsaa: make(width, height, SAMPLES),
      hdr: make(width, height),
      divisors,
      temps: divisors.map(() => null),
    };
    return this.post.targets;
  }

  /** Run the post passes from the scene (HDR + depth) into the canvas. */
  runPost(encoder, target, frame) {
    const post = this.post;
    const t = this.postTargets(this.size[0], this.size[1], frame.divisors);
    const texture = (code) => {
      if (code === 1) return t.hdr;
      const i = code - 3;
      if (!t.temps[i]) {
        const d = t.divisors[i];
        t.temps[i] = this.device.createTexture({
          size: [Math.max(1, Math.floor(t.width / d)), Math.max(1, Math.floor(t.height / d))],
          format: HDR,
          usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING,
        });
      }
      return t.temps[i];
    };
    const viewOf = (code) => (code === 0 ? post.placeholder : texture(code).createView());
    if (!post.uniforms || post.uniforms.size < frame.uniforms.byteLength) {
      post.uniforms?.destroy();
      post.uniforms = this.device.createBuffer({
        size: Math.ceil(frame.uniforms.byteLength / 256) * 256,
        usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
      });
    }
    this.device.queue.writeBuffer(post.uniforms, 0, frame.uniforms);
    const passBytes = frame.passes.length * 256;
    if (post.passCapacity < passBytes) {
      post.passBuffer?.destroy();
      post.passCapacity = Math.max(passBytes, 32 * 256);
      post.passBuffer = this.device.createBuffer({ size: post.passCapacity, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
    }
    const staging = new Uint8Array(passBytes);
    frame.passes.forEach((p, i) => staging.set(p.uniforms, i * 256));
    this.device.queue.writeBuffer(post.passBuffer, 0, staging);
    const depthView = this.depth.createView();
    frame.passes.forEach((p, i) => {
      const out = p.output === 2 ? target : texture(p.output).createView();
      const format = p.output === 2 ? this.viewFormat : HDR;
      const group = this.device.createBindGroup({
        layout: post.layout,
        entries: [
          { binding: 0, resource: { buffer: post.uniforms, size: frame.uniforms.byteLength } },
          { binding: 1, resource: { buffer: post.passBuffer, size: 256 } },
          { binding: 2, resource: post.sampler },
          { binding: 3, resource: viewOf(p.a) },
          { binding: 4, resource: viewOf(p.b) },
          { binding: 5, resource: depthView },
        ],
      });
      const pass = encoder.beginRenderPass({
        colorAttachments: [{ view: out, loadOp: "clear", storeOp: "store", clearValue: { r: 0, g: 0, b: 0, a: 1 } }],
      });
      pass.setPipeline(this.postPipeline(p.entry, format));
      pass.setBindGroup(0, group, [i * 256]);
      pass.draw(3);
      pass.end();
    });
  }

  makeTexture(width, height, mips) {
    const texture = this.device.createTexture({
      size: [width, height],
      format: "rgba8unorm-srgb",
      mipLevelCount: mips.length,
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
    });
    let [w, h] = [width, height];
    mips.forEach((data, level) => {
      this.device.queue.writeTexture({ texture, mipLevel: level }, data, { bytesPerRow: w * 4 }, [w, h]);
      w = Math.max(1, w >> 1);
      h = Math.max(1, h >> 1);
    });
    const group = this.device.createBindGroup({
      layout: this.textureLayout,
      entries: [
        { binding: 0, resource: texture.createView() },
        { binding: 1, resource: this.sampler },
      ],
    });
    return { texture, group };
  }

  resize(width, height) {
    if (this.size[0] === width && this.size[1] === height) return;
    this.size = [width, height];
    this.msaa?.destroy();
    this.depth?.destroy();
    this.msaa = this.device.createTexture({
      size: [width, height],
      sampleCount: SAMPLES,
      format: this.viewFormat,
      usage: GPUTextureUsage.RENDER_ATTACHMENT,
    });
    this.depth = this.device.createTexture({
      size: [width, height],
      sampleCount: SAMPLES,
      format: "depth32float",
      // Post-processing samples it (Focus, Ambient, "Objects inside" volumes).
      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING,
    });
  }

  ensureDrawBuffer(count) {
    if (count <= this.drawCapacity) return;
    this.drawCapacity = Math.max(64, 1 << Math.ceil(Math.log2(count)));
    this.drawBuffer?.destroy();
    this.drawBuffer = this.device.createBuffer({
      size: this.drawCapacity * DRAW_STRIDE,
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    });
    this.drawGroup = this.device.createBindGroup({
      layout: this.drawLayout,
      entries: [{ binding: 0, resource: { buffer: this.drawBuffer, size: DRAW_BYTES } }],
    });
    this.drawStaging = new Uint8Array(this.drawCapacity * DRAW_STRIDE);
  }

  /** Execute one frame's command buffer (bytes in wasm memory). */
  run(bytes) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const u32 = (at) => view.getUint32(at, true);
    const draws = [];
    let clear = [0, 0, 0, 1];
    let postFrame = null;
    let at = 0;
    while (at < bytes.length) {
      const op = u32(at);
      at += 4;
      if (op === OP.GEOMETRY) {
        const [id, layout, vertexCount, indexCount] = [u32(at), u32(at + 4), u32(at + 8), u32(at + 12)];
        at += 16;
        const vbytes = vertexCount * (layout === 0 ? 32 : 44);
        const vertices = this.device.createBuffer({ size: Math.max(vbytes, 4), usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
        this.device.queue.writeBuffer(vertices, 0, bytes.buffer, bytes.byteOffset + at, vbytes);
        at += vbytes;
        const ibytes = indexCount * 4;
        const indices = this.device.createBuffer({ size: Math.max(ibytes, 4), usage: GPUBufferUsage.INDEX | GPUBufferUsage.COPY_DST });
        this.device.queue.writeBuffer(indices, 0, bytes.buffer, bytes.byteOffset + at, ibytes);
        at += ibytes;
        this.geometry.set(id, { vertices, indices, count: indexCount });
      } else if (op === OP.FREE_GEOMETRY) {
        const g = this.geometry.get(u32(at));
        g?.vertices.destroy();
        g?.indices.destroy();
        this.geometry.delete(u32(at));
        at += 4;
      } else if (op === OP.TEXTURE) {
        const [id, width, height, levels] = [u32(at), u32(at + 4), u32(at + 8), u32(at + 12)];
        at += 16;
        const mips = [];
        let [w, h] = [width, height];
        for (let i = 0; i < levels; i++) {
          const n = w * h * 4;
          mips.push(bytes.slice(at, at + n));
          at += Math.ceil(n / 4) * 4;
          w = Math.max(1, w >> 1);
          h = Math.max(1, h >> 1);
        }
        this.textures.set(id, this.makeTexture(width, height, mips));
      } else if (op === OP.FRAME) {
        clear = [0, 1, 2, 3].map((i) => view.getFloat32(at + i * 4, true));
        this.device.queue.writeBuffer(this.frameBuffer, 0, bytes.buffer, bytes.byteOffset + at + 16, FRAME_BYTES);
        at += 16 + FRAME_BYTES;
      } else if (op === OP.POST) {
        const n = u32(at);
        at += 4;
        const uniforms = bytes.slice(at, at + n);
        at += n;
        const temps = u32(at);
        at += 4;
        const divisors = [];
        for (let i = 0; i < temps; i++) divisors.push(u32(at + i * 4));
        at += temps * 4;
        const count = u32(at);
        at += 4;
        const passes = [];
        for (let i = 0; i < count; i++) {
          const len = u32(at);
          at += 4;
          const entry = new TextDecoder().decode(bytes.subarray(at, at + len));
          at += Math.ceil(len / 4) * 4;
          passes.push({ entry, a: u32(at), b: u32(at + 4), output: u32(at + 8), uniforms: bytes.slice(at + 12, at + 12 + 256) });
          at += 12 + 256;
        }
        postFrame = { uniforms, divisors, passes };
      } else if (op === OP.DRAW) {
        draws.push({
          pass: u32(at),
          geometry: u32(at + 4),
          first: u32(at + 8),
          count: u32(at + 12),
          texture: u32(at + 16),
          uniforms: at + 20,
        });
        at += 20 + DRAW_BYTES;
      } else if (op === OP.END) {
        break;
      } else {
        throw new Error(`unknown Kit command ${op} at ${at - 4}`);
      }
    }

    this.ensureDrawBuffer(draws.length);
    draws.forEach((d, i) => this.drawStaging.set(bytes.subarray(d.uniforms, d.uniforms + DRAW_BYTES), i * DRAW_STRIDE));
    if (draws.length) this.device.queue.writeBuffer(this.drawBuffer, 0, this.drawStaging, 0, draws.length * DRAW_STRIDE);

    const encoder = this.device.createCommandEncoder();
    const target = this.context.getCurrentTexture().createView({ format: this.viewFormat });
    if (postFrame && !this.post) this.setupPost(this.postShaderCode);
    // With post-processing the scene goes to linear HDR (and keeps its depth).
    const postTargets = postFrame ? this.postTargets(this.size[0], this.size[1], postFrame.divisors) : null;
    const pipelines = this.pipelinesFor(postFrame ? HDR : this.viewFormat);
    const pass = encoder.beginRenderPass({
      colorAttachments: [
        {
          view: postFrame ? postTargets.hdrMsaa.createView() : this.msaa.createView(),
          resolveTarget: postFrame ? postTargets.hdr.createView() : target,
          clearValue: { r: clear[0], g: clear[1], b: clear[2], a: clear[3] },
          loadOp: "clear",
          storeOp: "discard",
        },
      ],
      depthStencilAttachment: {
        view: this.depth.createView(),
        depthClearValue: 1,
        depthLoadOp: "clear",
        depthStoreOp: postFrame ? "store" : "discard",
      },
    });
    pass.setBindGroup(0, this.frameGroup);
    let current = -1;
    draws.forEach((d, i) => {
      const g = this.geometry.get(d.geometry);
      if (!g) return;
      if (d.pass !== current) {
        pass.setPipeline(pipelines[d.pass]);
        current = d.pass;
      }
      pass.setBindGroup(1, (this.textures.get(d.texture) ?? this.textures.get(0)).group);
      pass.setBindGroup(2, this.drawGroup, [i * DRAW_STRIDE]);
      pass.setVertexBuffer(0, g.vertices);
      pass.setIndexBuffer(g.indices, "uint32");
      const count = d.count === 0xffffffff ? g.count : d.count;
      pass.drawIndexed(count, 1, d.first, 0, 0);
    });
    pass.end();
    if (postFrame) this.runPost(encoder, target, postFrame);
    this.device.queue.submit([encoder.finish()]);
    this.postPasses = postFrame ? postFrame.passes.length : 0;
    return draws.length;
  }
}

// ---------------------------------------------------------------- input

function connectInput(game, canvas) {
  const x = game.x;
  const key = (e, down) => {
    if (e.repeat) return;
    // Keep arrows/space from scrolling the page while playing.
    if (["Space", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Tab"].includes(e.code)) e.preventDefault();
    game.withBytes(new TextEncoder().encode(e.code), (p, n) => x.kit_key(p, n, down ? 1 : 0));
  };
  window.addEventListener("keydown", (e) => key(e, true));
  window.addEventListener("keyup", (e) => key(e, false));
  const pos = (e) => {
    const r = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    x.kit_mouse_move((e.clientX - r.left) * dpr, (e.clientY - r.top) * dpr);
  };
  canvas.addEventListener("pointermove", pos);
  canvas.addEventListener("pointerdown", (e) => {
    canvas.focus();
    canvas.setPointerCapture(e.pointerId);
    pos(e);
    x.kit_mouse_button(e.button, 1);
  });
  canvas.addEventListener("pointerup", (e) => {
    pos(e);
    x.kit_mouse_button(e.button, 0);
  });
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      // Lines, positive = away from the user (like desktop).
      const lines = e.deltaMode === 0 ? e.deltaY / 100 : e.deltaMode === 1 ? e.deltaY : e.deltaY * 10;
      x.kit_wheel(-lines);
    },
    { passive: false },
  );
  window.addEventListener("blur", () => x.kit_blur());
}

// ---------------------------------------------------------------- sound

/**
 * Game audio: kit_web mixes it (the same mixer as desktop), and this queues
 * short buffers back to back on a Web Audio timeline. Browsers only allow
 * sound after the player clicks or presses a key, so it starts then.
 */
class Sound {
  constructor(game) {
    this.game = game;
    this.ctx = null;
    this.next = 0;
  }
  start() {
    if (this.ctx) {
      if (this.ctx.state === "suspended") this.ctx.resume();
      return;
    }
    const AC = window.AudioContext || window.webkitAudioContext;
    if (!AC) return;
    this.ctx = new AC({ latencyHint: "interactive" });
    this.next = 0;
  }
  /** Keep ~0.12 s of audio queued ahead. Called every frame. */
  pump() {
    const ctx = this.ctx;
    if (!ctx || ctx.state !== "running") return;
    const CHUNK = 1024;
    if (this.next < ctx.currentTime) this.next = ctx.currentTime + 0.03;
    while (this.next < ctx.currentTime + 0.12) {
      const ptr = this.game.x.kit_audio_mix(CHUNK, ctx.sampleRate);
      const data = new Float32Array(this.game.x.memory.buffer, ptr, CHUNK * 2);
      const buffer = ctx.createBuffer(2, CHUNK, ctx.sampleRate);
      const left = buffer.getChannelData(0);
      const right = buffer.getChannelData(1);
      for (let i = 0; i < CHUNK; i++) {
        left[i] = data[i * 2];
        right[i] = data[i * 2 + 1];
      }
      const node = ctx.createBufferSource();
      node.buffer = buffer;
      node.connect(ctx.destination);
      node.start(this.next);
      this.next += CHUNK / ctx.sampleRate;
    }
  }
}

// ----------------------------------------------------------------- main

async function main() {
  if (!navigator.gpu) {
    fail("Turn on WebGPU to play", "This game draws its 3D graphics with WebGPU, which is switched off in this browser.", webgpuHelp(false));
    return;
  }
  const adapter = await navigator.gpu.requestAdapter({ powerPreference: "high-performance" });
  if (!adapter) {
    fail("Turn on WebGPU to play", "This browser supports WebGPU but didn't allow it on this computer's graphics card.", webgpuHelp(true));
    return;
  }
  const device = await adapter.requestDevice();
  device.lost.then((info) => {
    if (info.reason !== "destroyed") fail("The graphics device was lost", info.message);
  });
  window.kitGpuErrors = [];
  device.addEventListener("uncapturederror", (e) => {
    console.error("WebGPU:", e.error.message);
    window.kitGpuErrors.push(e.error.message);
  });

  let progress = [0, 0];
  const show = () => {
    const p = (progress[0] * 0.2 + progress[1] * 0.8) * 100;
    ui.bar.style.width = `${p.toFixed(1)}%`;
  };
  const packFile = KIT_TRUST ? "game.kitseal" : "game.pack";
  let wasmBytes, pack;
  try {
    [wasmBytes, pack] = await Promise.all([
      fetchWithProgress("kit_web.wasm", (p) => ((progress[0] = p), show())),
      fetchWithProgress(packFile, (p) => ((progress[1] = p), show())),
    ]);
  } catch (e) {
    fail("The game couldn't load", KIT_TRUST ? `Its signed game data (${packFile}) is missing.` : String(e?.message ?? e));
    return;
  }
  ui.status.textContent = "Starting…";
  const { instance } = await WebAssembly.instantiate(wasmBytes, {});
  const game = new Game(instance);
  const seed = Math.random() * 2 ** 52;
  const ok = KIT_TRUST
    ? KIT_TRUST.mode === "signed" &&
      game.withBytes(new Uint8Array(KIT_TRUST.publisher), (kp, kn) =>
        game.withBytes(pack, (p, n) => game.x.kit_start_sealed(p, n, kp, kn, seed)),
      )
    : game.withBytes(pack, (p, n) => game.x.kit_start(p, n, seed));
  if (!ok) {
    fail("The game couldn't start", game.text());
    game.drainLogs();
    return;
  }
  document.title = game.text();
  game.drainLogs();

  const context = ui.canvas.getContext("webgpu");
  const format = navigator.gpu.getPreferredCanvasFormat();
  const viewFormat = format.endsWith("-srgb") ? format : `${format}-srgb`;
  context.configure({ device, format, viewFormats: [viewFormat], alphaMode: "opaque" });
  const renderer = new Renderer(device, context, format, game.shader());
  renderer.postShaderCode = game.postShader();
  connectInput(game, ui.canvas);
  const sound = (window.kitSound = new Sound(game));
  for (const type of ["pointerdown", "keydown", "touchstart"]) {
    window.addEventListener(type, () => sound.start(), { capture: true });
  }
  ui.loading.hidden = true;
  ui.canvas.focus();

  const stats = (window.kitStats = { frames: 0, draws: 0, errors: [] });
  let last = performance.now();
  const frame = (now) => {
    const dt = Math.min(Math.max((now - last) / 1000, 0), 0.25);
    last = now;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(ui.canvas.clientWidth * dpr));
    const h = Math.max(1, Math.round(ui.canvas.clientHeight * dpr));
    if (ui.canvas.width !== w || ui.canvas.height !== h) {
      ui.canvas.width = w;
      ui.canvas.height = h;
    }
    renderer.resize(w, h);
    try {
      game.x.kit_frame(dt, w, h);
      const ptr = game.x.kit_frame_ptr();
      const len = game.x.kit_frame_len();
      stats.draws = renderer.run(game.bytes.subarray(ptr, ptr + len));
      stats.postPasses = renderer.postPasses;
    } catch (e) {
      game.drainLogs();
      fail("The game stopped", String(e));
      return;
    }
    sound.pump();
    stats.errors.push(...game.drainLogs());
    stats.frames++;
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}

main().catch((e) => fail("The game couldn't start", String(e?.message ?? e)));
