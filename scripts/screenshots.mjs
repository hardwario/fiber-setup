#!/usr/bin/env node
// Regenerates the screenshots in docs/images (the ones in the README) from the browser preview:
// the app's own screens with a simulated FIBER, in the light theme. It walks the wizard the way
// a user does, once with one FIBER and once with four, and takes a picture where a shot says so.
// Needs Google Chrome or Chromium installed.
//
//   npm run screenshots                  all of them
//   npm run screenshots -- write done    only some
//
// CHROME_PATH=/path/to/chrome overrides the browser.

import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { createServer } from "vite";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "docs/images");
const VIEWPORT = { width: 1200, height: 800 };

/** The pictures: the step each is taken on, and with how many simulated FIBERs. */
const SHOTS = [
  { name: "settings", boards: 1, at: "settings" },
  { name: "boot-mode", boards: 1, at: "jumper" },
  { name: "connect", boards: 1, at: "connect" },
  { name: "write", boards: 1, at: "write" },
  { name: "done", boards: 1, at: "done" },
  { name: "several-connect", boards: 4, at: "connect" },
  { name: "several-write", boards: 4, at: "write" },
];

const asked = process.argv.slice(2);
const unknown = asked.filter((a) => !SHOTS.some((s) => s.name === a));
if (unknown.length) {
  console.error(`No such shot: ${unknown.join(", ")}. There are: ${SHOTS.map((s) => s.name).join(", ")}`);
  process.exit(2);
}
const shots = asked.length ? SHOTS.filter((s) => asked.includes(s.name)) : SHOTS;

/** Runs `fn` in the page with the wizard (the preview puts it on `window`). */
const wizard = (page, fn, arg) => page.evaluate(`(${fn})(window.__wizard, ${JSON.stringify(arg ?? null)})`);
const until = (page, fn, timeout = 60000) =>
  page.waitForFunction(`(${fn})(window.__wizard)`, null, { timeout, polling: 200 });

/** Each step of the walk: what brings the wizard there and when the page is worth a picture. */
const WALK = [
  { step: "image", ready: (w) => !!w.image.result && !w.image.busy },
  { step: "settings", ready: () => true },
  { step: "jumper", ready: () => true },
  {
    step: "connect",
    enter: (w) => w.allowAccess(),
    // Every simulated FIBER is on USB (they come up one after another).
    ready: (w) => w.boards.length === Number(new URLSearchParams(location.search).get("boards")),
  },
  {
    step: "write",
    enter: (w) => w.startFlash(),
    // Every FIBER a good way into its write.
    ready: (w) => {
      const jobs = Object.values(w.jobs);
      return jobs.length > 0 && jobs.every((j) => j.phase === "write" && j.total > 0 && j.done / j.total > 0.35);
    },
    leave: (w) => !w.writing && w.done.length > 0,
  },
  { step: "restart", ready: (w) => w.boards.length === 0, leave: (w) => w.boards.length === 0 },
  { step: "start", ready: (w) => w.names.every((n) => w.found[n]) },
  { step: "done", ready: () => true },
];

async function capture(page, cdp, shot) {
  await page.mouse.move(0, VIEWPORT.height - 1);
  // The size is set here, straight in Chrome, and not only through the context's viewport:
  // headless Chrome's own window is lower than the viewport asked for, and a capture would stop
  // at its edge. Twice the pixels keep the pictures sharp on a dense display.
  await cdp.send("Emulation.setDeviceMetricsOverride", { ...VIEWPORT, deviceScaleFactor: 2, mobile: false });
  await page.waitForTimeout(400);
  // WebP straight from Chrome keeps the files small without extra tools.
  const { data } = await cdp.send("Page.captureScreenshot", { format: "webp", quality: 82 });
  const file = join(OUT, `${shot.name}.webp`);
  await writeFile(file, Buffer.from(data, "base64"));
  console.log(file.slice(ROOT.length + 1));
}

const server = await createServer({
  root: ROOT,
  logLevel: "error",
  // Its own port, so it also runs next to `npm run dev` or `npm run tauri dev`.
  server: { port: 1440, strictPort: false },
});
await server.listen();
const base = server.resolvedUrls?.local[0] ?? "http://localhost:1440/";

const browser = await chromium.launch(
  process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: "chrome" },
);

try {
  await mkdir(OUT, { recursive: true });
  for (const boards of [...new Set(shots.map((s) => s.boards))]) {
    const mine = shots.filter((s) => s.boards === boards);
    const context = await browser.newContext({ viewport: VIEWPORT, deviceScaleFactor: 2, colorScheme: "light" });
    // English, and the theme the browser asks for.
    await context.addInitScript(() =>
      localStorage.setItem("fiber-setup.prefs", JSON.stringify({ theme: "system", language: "en" })),
    );
    const page = await context.newPage();
    const cdp = await context.newCDPSession(page);
    await page.goto(`${base}?boards=${boards}`);
    await until(page, (w) => !!w?.info);
    // The preview's own label is not part of the app.
    await page.addStyleTag({ content: ".sim { display: none !important; }" });

    const last = Math.max(...mine.map((s) => WALK.findIndex((step) => step.step === s.at)));
    for (const stage of WALK.slice(0, last + 1)) {
      await wizard(page, (w) => w.next());
      await until(page, `(w) => w.step === ${JSON.stringify(stage.step)}`);
      if (stage.enter) await wizard(page, stage.enter);
      await until(page, stage.ready);
      for (const shot of mine.filter((s) => s.at === stage.step)) await capture(page, cdp, shot);
      if (stage.leave) await until(page, stage.leave, 120000);
    }
    await context.close();
  }
} finally {
  await browser.close();
  await server.close();
}
