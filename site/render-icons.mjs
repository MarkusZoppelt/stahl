// Renders the steel door with the landing page's own shader (site/door.js) at 1024 px, as raw
// straight-alpha RGBA. site/render-icons.sh turns it into every icon the app and the page use.
//
//   node site/render-icons.mjs <page-url> <out.rgba>
import { chromium } from "playwright";
import { writeFileSync } from "node:fs";

const [url, out] = process.argv.slice(2);
const size = 1024;
const browser = await chromium.launch({
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ bypassCSP: true })).newPage();
await page.goto(url, { waitUntil: "load" });
const premultiplied = await page.evaluate((size) => {
  const canvas = document.createElement("canvas");
  const door = window.createDoor(canvas);
  // The page's resting light, and no split: the closed door, as the hero first shows it.
  door.draw({ width: size, height: size, scale: 1, x: size / 2, y: size / 2, radius: size / 2 - 2, split: 0, spin: 0, lightX: -0.55, lightY: -0.65, gain: 1 });
  const gl = canvas.getContext("webgl2");
  const pixels = new Uint8Array(size * size * 4);
  gl.readPixels(0, 0, size, size, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
  return Array.from(pixels);
}, size);
await browser.close();

// GL rows start at the bottom and are premultiplied. Flip them, and divide alpha out.
const rgba = new Uint8Array(size * size * 4);
for (let y = 0; y < size; y++) {
  for (let x = 0; x < size; x++) {
    const from = ((size - 1 - y) * size + x) * 4;
    const to = (y * size + x) * 4;
    const alpha = premultiplied[from + 3];
    for (let k = 0; k < 3; k++) rgba[to + k] = alpha ? Math.min(255, Math.round((premultiplied[from + k] * 255) / alpha)) : 0;
    rgba[to + 3] = alpha;
  }
}
writeFileSync(out, rgba);
