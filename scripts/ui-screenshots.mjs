// Screenshots of interactive UI (menus, the parts browser) for the docs.
// Drives a headless Chromium over the DevTools protocol:
//   node scripts/ui-screenshots.mjs <devtools port> <app url> <out dir>
// Writes PNGs; scripts/screenshots.sh converts them to WebP.
import { writeFileSync } from "node:fs";

const [port, url, out] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let targets;
for (let i = 0; i < 100 && !targets; i++) {
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

const shot = async (name, clip) => {
  const r = await send("Page.captureScreenshot", { format: "png", ...(clip && { clip: { ...clip, scale: 1 } }) });
  writeFileSync(`${out}/${name}.png`, Buffer.from(r.data, "base64"));
  console.log(`  ${name}`);
};
const move = (x, y) => send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
const click = async (x, y) => {
  await move(x, y);
  for (const type of ["mousePressed", "mouseReleased"]) {
    await send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  }
};
const key = async (key, code, keyCode, modifiers = 0) => {
  for (const type of ["keyDown", "keyUp"]) {
    await send("Input.dispatchKeyEvent", { type, key, code, windowsVirtualKeyCode: keyCode, modifiers });
  }
};

await send("Emulation.setDeviceMetricsOverride", { width: 1400, height: 900, deviceScaleFactor: 1, mobile: false });
await send("Page.navigate", { url });
await sleep(6000);

// The library on the left, folders closed except the first.
await shot("library", { x: 0, y: 44, width: 236, height: 440 });

// Shift+A at (600, 250), then hover "Electronics & I/O" to open its list.
await move(600, 250);
await sleep(200);
await key("A", "KeyA", 65, 8);
await sleep(400);
for (let y = 300; y <= 352; y += 4) {
  await move(640, y);
  await sleep(40);
}
await sleep(400);
await shot("add-menu", { x: 570, y: 225, width: 580, height: 260 });
await key("Escape", "Escape", 27);
await sleep(300);

// The parts browser, opened with the ▦ button next to the search box.
await click(18, 62);
await sleep(700);
await shot("browser", { x: 320, y: 210, width: 760, height: 480 });

ws.close();
