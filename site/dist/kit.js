const shots = {
  editor: ["Kit’s scene hierarchy, 3D viewport, and inspector", "Scene editing in Kit’s native editor."],
  canvas: ["A sketched movement path in the Kit viewport", "Canvas mode turns a drawn route into object movement."],
  terrain: ["Terrain sculpting tools open in the Kit viewport", "Sculpt terrain without leaving the editor."],
  "script-editor": ["An x gameplay script open in the Kit code editor", "Write gameplay in x in the built-in script editor."],
  "turtle-editor": ["The textured FORGE turtle imported into the Kit editor", "A textured FORGE turtle imported as GLB and placed in a Kit scene."]
};
const screenshot = document.querySelector("#editor-shot");
const caption = document.querySelector("#shot-caption");
const screenshotButtons = [...document.querySelectorAll("[data-shot]")];
for (const button of screenshotButtons) {
  button.addEventListener("click", () => {
    const [alt, description] = shots[button.dataset.shot];
    screenshot.src = `assets/${button.dataset.shot}.webp`;
    screenshot.alt = alt;
    caption.textContent = description;
    for (const item of screenshotButtons) item.setAttribute("aria-pressed", String(item === button));
  });
}
const dialog = document.querySelector("#shot-dialog");
document.querySelector(".expand").addEventListener("click", () => {
  const image = dialog.querySelector("img");
  image.src = screenshot.src;
  image.alt = screenshot.alt;
  dialog.showModal();
});
dialog.querySelector("button").addEventListener("click", () => dialog.close());
dialog.addEventListener("click", event => { if (event.target === dialog) dialog.close(); });

const canvas = document.querySelector("#path-board");
const context = canvas.getContext("2d");
const play = document.querySelector("#play-path");
const status = document.querySelector("#path-status");
const example = [[70,308],[145,273],[209,298],[285,214],[352,184],[418,226],[502,151],[620,101]];
let points = example.slice();
let marker = points[0];
let stroke = null;
let animation = 0;

function render() {
  const {width, height} = canvas;
  context.clearRect(0, 0, width, height);
  context.fillStyle = "#181123";
  context.fillRect(0, 0, width, height);
  context.strokeStyle = "#3a284b";
  context.lineWidth = 1;
  context.beginPath();
  for (let x = 14; x < width; x += 28) { context.moveTo(x, 0); context.lineTo(x, height); }
  for (let y = 9; y < height; y += 28) { context.moveTo(0, y); context.lineTo(width, y); }
  context.stroke();
  context.strokeStyle = "#c995fb";
  context.lineWidth = 3;
  context.lineJoin = "round";
  context.lineCap = "round";
  context.shadowColor = "#ae60ff";
  context.shadowBlur = 13;
  context.beginPath();
  points.forEach(([x, y], index) => index ? context.lineTo(x, y) : context.moveTo(x, y));
  context.stroke();
  context.shadowBlur = 0;
  context.fillStyle = "#ead4ff";
  const nodes = points === example ? points : [points[0], points.at(-1)];
  for (const [x, y] of nodes) context.fillRect(x - 3, y - 3, 6, 6);
  context.fillStyle = "#ffa88c";
  context.strokeStyle = "#fff0dc";
  context.lineWidth = 2;
  context.beginPath();
  context.arc(...marker, 9, 0, Math.PI * 2);
  context.fill();
  context.stroke();
}

function stop() {
  cancelAnimationFrame(animation);
  play.disabled = false;
}

function coordinates(event) {
  const bounds = canvas.getBoundingClientRect();
  const x = (event.clientX - bounds.left) * canvas.width / bounds.width;
  const y = (event.clientY - bounds.top) * canvas.height / bounds.height;
  return [Math.min(canvas.width - 12, Math.max(12, x)), Math.min(canvas.height - 12, Math.max(12, y))];
}

canvas.addEventListener("pointerdown", event => {
  if (stroke !== null || event.button !== 0) return;
  stop();
  stroke = event.pointerId;
  canvas.setPointerCapture(stroke);
  points = [coordinates(event)];
  marker = points[0];
  play.disabled = true;
  status.textContent = "Drawing a route…";
  render();
});
canvas.addEventListener("pointermove", event => {
  if (stroke !== event.pointerId) return;
  const point = coordinates(event);
  const last = points.at(-1);
  if (Math.hypot(point[0] - last[0], point[1] - last[1]) >= 4) points.push(point);
  render();
});
function finishStroke(event) {
  if (stroke !== event.pointerId) return;
  stroke = null;
  play.disabled = points.length < 2;
  status.textContent = play.disabled ? "Drag across the grid to draw a route." : "Route ready. Press Play path.";
}
canvas.addEventListener("pointerup", finishStroke);
canvas.addEventListener("pointercancel", finishStroke);
document.querySelector("#reset-path").addEventListener("click", () => {
  stop();
  stroke = null;
  points = example;
  marker = points[0];
  status.textContent = "Try the example route or drag to draw your own.";
  render();
});
play.addEventListener("click", () => {
  if (points.length < 2) return;
  const distances = [0];
  for (let i = 1; i < points.length; i++) {
    distances.push(distances[i - 1] + Math.hypot(points[i][0] - points[i - 1][0], points[i][1] - points[i - 1][1]));
  }
  const total = distances.at(-1);
  const started = performance.now();
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let segment = 1;
  play.disabled = true;
  status.textContent = "Following your path…";
  function advance(now) {
    const distance = reducedMotion ? total : Math.min(total, (now - started) * 0.27);
    while (segment < points.length - 1 && distances[segment] < distance) segment++;
    const t = (distance - distances[segment - 1]) / (distances[segment] - distances[segment - 1]);
    marker = points[segment - 1].map((value, axis) => value + t * (points[segment][axis] - value));
    render();
    if (distance < total) animation = requestAnimationFrame(advance);
    else {
      play.disabled = false;
      status.textContent = "Route complete.";
    }
  }
  animation = requestAnimationFrame(advance);
});
points = example;
render();
