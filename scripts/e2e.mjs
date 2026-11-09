// End-to-end tests of the web app in a real browser, driven with real mouse
// events over the DevTools protocol. Checks what's on screen by its pixels.
//   node scripts/e2e.mjs <devtools port> <site url> [<failure screenshot dir>]
// Run through scripts/e2e.sh, which serves the built site and starts Chromium.
import { writeFileSync, mkdirSync } from "node:fs";
import { inflateSync } from "node:zlib";

const [port, site, out = "e2e-failures"] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let targets;
for (let i = 0; i < 200 && !targets; i++) {
  try {
    targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  } catch {
    await sleep(100);
  }
}
const page = targets.find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0;
const pending = new Map();
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  pending.get(d.id)?.(d.result);
  pending.delete(d.id);
};
const send = (method, params = {}) =>
  new Promise((r) => {
    pending.set(++id, r);
    ws.send(JSON.stringify({ id, method, params }));
  });

// ---- input ---------------------------------------------------------------

const mouse = (type, x, y, extra = {}) => send("Input.dispatchMouseEvent", { type, x, y, ...extra });
const move = async (x, y) => {
  await mouse("mouseMoved", x, y);
  await sleep(120);
};
const click = async (x, y) => {
  await move(x, y);
  await mouse("mousePressed", x, y, { button: "left", buttons: 1, clickCount: 1 });
  await mouse("mouseReleased", x, y, { button: "left", clickCount: 1 });
  await sleep(300);
};
/** Press at (x1, y1), move in steps to (x2, y2), release. */
const drag = async (x1, y1, x2, y2) => {
  await move(x1, y1);
  await mouse("mousePressed", x1, y1, { button: "left", buttons: 1, clickCount: 1 });
  await sleep(80);
  for (let k = 1; k <= 20; k++) {
    await mouse("mouseMoved", x1 + ((x2 - x1) * k) / 20, y1 + ((y2 - y1) * k) / 20, { button: "left", buttons: 1 });
    await sleep(25);
  }
  await sleep(150);
  await mouse("mouseReleased", x2, y2, { button: "left", clickCount: 1 });
  await sleep(400);
};

// ---- screen --------------------------------------------------------------

/** Decodes an 8-bit RGB/RGBA PNG (what Chromium's screenshots are). */
function decodePng(buf) {
  let pos = 8;
  let width, height, channels;
  const idat = [];
  while (pos < buf.length) {
    const len = buf.readUInt32BE(pos);
    const type = buf.toString("ascii", pos + 4, pos + 8);
    const data = buf.subarray(pos + 8, pos + 8 + len);
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      channels = { 2: 3, 6: 4 }[data[9]];
      if (data[8] !== 8 || !channels) throw new Error("unsupported PNG");
    } else if (type === "IDAT") idat.push(data);
    pos += 12 + len;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const px = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x++) {
      const a = x >= channels ? px[y * stride + x - channels] : 0;
      const b = y > 0 ? px[(y - 1) * stride + x] : 0;
      const c = x >= channels && y > 0 ? px[(y - 1) * stride + x - channels] : 0;
      const p = a + b - c;
      const pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
      const paeth = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      const v = line[x];
      px[y * stride + x] = (filter === 0 ? v : filter === 1 ? v + a : filter === 2 ? v + b : filter === 3 ? v + ((a + b) >> 1) : v + paeth) & 255;
    }
  }
  return { width, channels, px, png: buf };
}

let screen;
const capture = async () => {
  const r = await send("Page.captureScreenshot", { format: "png" });
  screen = decodePng(Buffer.from(r.data, "base64"));
};
const pixel = (x, y) => {
  const i = (y * screen.width + x) * screen.channels;
  return [screen.px[i], screen.px[i + 1], screen.px[i + 2]];
};
const red = ([r, g, b]) => r > 170 && g < 100 && b < 100;
/** Whether any pixel in the box around (x, y) is red (light at 650 nm). */
const lightNear = (x, y, r = 3) => {
  for (let dy = -r; dy <= r; dy++) for (let dx = -r; dx <= r; dx++) if (red(pixel(x + dx, y + dy))) return true;
  return false;
};

// ---- tests ---------------------------------------------------------------

const results = [];
async function test(name, body) {
  try {
    await body();
    results.push([name, null]);
    console.log(`ok   ${name}`);
  } catch (e) {
    results.push([name, e]);
    console.log(`FAIL ${name}: ${e.message}`);
    mkdirSync(out, { recursive: true });
    if (screen) writeFileSync(`${out}/${name.replace(/\W+/g, "-")}.png`, screen.png);
  }
}
const expect = (ok, message) => {
  if (!ok) throw new Error(message);
};

await send("Emulation.setDeviceMetricsOverride", { width: 1400, height: 900, deviceScaleFactor: 1, mobile: false });
const open = async (query = "") => {
  await send("Page.navigate", { url: `${site}/app/${query}` });
  // Wait for the app to draw its menu bar (dark text top left).
  for (let i = 0; i < 100; i++) {
    await sleep(200);
    await capture();
    const [r, g, b] = pixel(14, 12);
    if (r + g + b < 300) return;
  }
  throw new Error("the app didn't start");
};

// Layout at 1400×900 and 100% zoom: the grid's cells are 40 points.
const LASER_IN_LIBRARY = [75, 111];
const WAVEGUIDE_FOLDER = [78, 295];
const MZI_IN_LIBRARY = [90, 419];

await test("a laser dragged onto the grid shines a beam", async () => {
  await open();
  await drag(...LASER_IN_LIBRARY, 450, 330);
  await move(900, 700);
  await capture();
  expect(lightNear(900, 311), "no beam to the right of the laser");
});

await test("a laser wired to an MZI lights both of its outputs", async () => {
  // Continues from the previous test: the laser is at (451, 311).
  await click(...WAVEGUIDE_FOLDER);
  await drag(...MZI_IN_LIBRARY, 690, 470);
  await capture();
  expect(lightNear(900, 311), "the laser's beam should still pass above the MZI");
  expect(!lightNear(900, 471) && !lightNear(900, 511), "the MZI shouldn't be lit yet");
  // From the laser's light output (its right edge) to the MZI's in1.
  await drag(471, 311, 671, 471);
  await move(900, 700);
  await capture();
  expect(!lightNear(900, 311), "the beam should go into the waveguide instead");
  expect(lightNear(900, 471), "no light from out1");
  expect(lightNear(900, 511), "no light from out2");
  let wired = false;
  for (let y = 315; y < 470 && !wired; y += 2) wired = lightNear(570, y, 1);
  expect(wired, "no lit waveguide between the laser and the MZI");
});

await test("an example opens from a link and shows its light", async () => {
  await open("?example=mach-zehnder-interferometer");
  let lit = 0;
  for (let y = 60; y < 860; y += 4) for (let x = 240; x < 1100; x += 4) if (red(pixel(x, y))) lit++;
  expect(lit > 50, `only ${lit} lit samples`);
});

ws.close();
const failed = results.filter(([, e]) => e);
console.log(`\n${results.length - failed.length} passed, ${failed.length} failed`);
process.exit(failed.length ? 1 : 0);
