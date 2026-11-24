import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import puppeteer from "puppeteer";
import { openTab } from "./helpers.mjs";

const [tab, out] = process.argv.slice(2);
const url = process.env.RHEBOK_URL;
if (!tab || !out || !url) {
  console.error("usage: RHEBOK_URL=<dashboard url> node shoot-web.mjs <tab> <out.png>");
  process.exit(2);
}

const options = { headless: "shell", args: ["--no-sandbox"] };
if (process.env.RHEBOK_E2E_CHROME) options.executablePath = process.env.RHEBOK_E2E_CHROME;
const browser = await puppeteer.launch(options);
try {
  const page = await browser.newPage();
  await page.setViewport({ width: Number(process.env.RHEBOK_PARITY_WIDTH ?? 1400), height: 900, deviceScaleFactor: 2 });
  const graph = tab === "graph" ? process.env.RHEBOK_PARITY_GRAPH : undefined;
  if (graph === "error") {
    await page.setRequestInterception(true);
    page.on("request", (request) => {
      if (new URL(request.url()).pathname === "/api/graph") {
        request.respond({ status: 503, contentType: "application/json", body: "{}" });
      } else {
        request.continue();
      }
    });
  }
  const onboarding = tab === "onboarding" ? (process.env.RHEBOK_PARITY_ONBOARDING ?? "folders") : undefined;
  if (onboarding === "index") {
    await page.setRequestInterception(true);
    page.on("request", (request) => {
      const path = new URL(request.url()).pathname;
      if (request.method() === "POST" && path === "/api/index") {
        request.respond({ status: 202, contentType: "application/json", body: JSON.stringify({ started: true, pid: 1, reindex: false }) });
      } else if (request.method() === "GET" && path === "/api/index") {
        request.respond({ status: 200, contentType: "application/json", body: JSON.stringify({ run: null }) });
      } else if (path === "/api/progress") {
        request.respond({ status: 200, contentType: "application/json", body: JSON.stringify({ idle: true }) });
      } else {
        request.continue();
      }
    });
  }
  await page.goto(url, { waitUntil: "networkidle0" });
  if (onboarding) {
    await page.waitForSelector("[data-testid=onboarding]", { timeout: 20_000 });
  } else {
    await openTab(page, tab[0].toUpperCase() + tab.slice(1));
  }
  await page.waitForNetworkIdle({ idleTime: 500 });
  if (onboarding) {
    const press = async (text) => {
      const handle = await page.waitForSelector(`xpath/.//button[normalize-space()='${text}' and not(@disabled)]`, { timeout: 30_000 });
      await handle.click();
    };
    const folder = process.env.RHEBOK_PARITY_FOLDER;
    if (onboarding === "added" || onboarding === "search") {
      if (!folder) throw new Error("RHEBOK_PARITY_FOLDER is not set");
      await page.type("input[aria-label='folder to index']", folder);
      await page.waitForFunction(() => !document.body.innerText.includes("checking…"));
      await press("Add folder");
      await page.waitForFunction((f) => [...document.querySelectorAll("[data-testid=setup-folders] [data-item]")].some((e) => e.dataset.item === f), {}, folder);
    }
    if (onboarding === "connect" || onboarding === "index") await press("continue without saving");
    if (onboarding === "search") await press("Save and continue");
    if (onboarding !== "folders" && onboarding !== "added") {
      await page.waitForFunction(() => document.querySelector("[data-agent]") || document.body.innerText.includes("No coding agent"), { timeout: 30_000 });
      await page.waitForNetworkIdle({ idleTime: 500 });
    }
    if (onboarding === "index") {
      await press("Continue");
      await page.waitForSelector("[data-testid=index-progress]");
    }
    if (onboarding === "search") {
      await press("Continue");
      await page.waitForSelector("[data-testid=suggested-query]", { timeout: 120_000 });
    }
    await page.waitForNetworkIdle({ idleTime: 500 });
  }
  const query = process.env.RHEBOK_PARITY_QUERY;
  if (tab === "search" && query) {
    const input = await page.waitForSelector("input");
    await input.type(query);
    await page.keyboard.press("Enter");
    await page.waitForNetworkIdle({ idleTime: 500 });
    if (process.env.RHEBOK_PARITY_EXPAND) {
      const toggle = await page.waitForSelector("button[aria-expanded]");
      await toggle.click();
      await page.waitForNetworkIdle({ idleTime: 500 });
    }
  }
  const memories = process.env.RHEBOK_PARITY_MEMORIES;
  if (tab === "memories" && ["open", "confirm", "long"].includes(memories)) {
    const row = await page.waitForSelector("xpath/.//button[.//span[@title]]");
    await row.click();
    await page.waitForNetworkIdle({ idleTime: 500 });
    if (memories === "confirm") {
      const del = await page.waitForSelector("xpath/.//button[normalize-space()='Delete']");
      await del.click();
    }
  }
  if (tab === "memories" && memories === "new") {
    const create = await page.waitForSelector("xpath/.//button[normalize-space()='New']");
    await create.click();
  }
  const integrations = process.env.RHEBOK_PARITY_INTEGRATIONS;
  if (tab === "integrations" && integrations) {
    const click = async (text, agent) => {
      const button = await page.waitForSelector(`xpath/.//*[@data-agent='${agent}']//button[normalize-space()='${text}' and not(@disabled)]`);
      await button.click();
      await page.waitForNetworkIdle({ idleTime: 500 });
    };
    if (integrations === "open") await page.click("[data-testid=not-installed] summary");
    if (integrations === "confirm" || integrations === "disconnect") await click("Disconnect", "cursor");
    if (integrations === "disconnect") await click("Disconnect", "cursor");
    if (integrations === "change") await click("Connect", "cursor");
    if (integrations === "refusal") await click("Repair", "gemini");
    if (integrations === "snippets") {
      await page.$eval("[data-testid=integrations]", (el) => (el.scrollTop = el.scrollHeight));
    }
  }
  const settings = process.env.RHEBOK_PARITY_SETTINGS;
  if (tab === "settings" && settings) {
    const button = async (text, within) => {
      const handle = await page.waitForSelector(
        `xpath/.//*[@data-testid='${within}']//button[normalize-space()='${text}' and not(@disabled)]`,
      );
      await handle.click();
    };
    const checked = () =>
      page.waitForFunction(() => {
        const bar = document.querySelector("[data-testid=save-bar]");
        return bar && !bar.innerText.includes("checking");
      });
    if (settings === "reindex") {
      await page.setRequestInterception(true);
      page.on("request", (request) => {
        const path = new URL(request.url()).pathname;
        if (request.method() === "GET" && path === "/api/index") {
          request.respond({ status: 200, contentType: "application/json", body: JSON.stringify({ run: null }) });
        } else if (path === "/api/progress") {
          request.respond({ status: 200, contentType: "application/json", body: JSON.stringify({ idle: true }) });
        } else {
          request.continue();
        }
      });
      await button("Re-index now", "settings");
      await page.waitForSelector("[data-testid=index-progress]");
      await page.$eval("[data-testid=settings] .overflow-auto", (el) => (el.scrollTop = el.scrollHeight));
    }
    if (settings === "notice") {
      await button("Embedding", "settings");
      const model = await page.waitForSelector('input[aria-label="embed.model"]');
      await model.click({ count: 3 });
      await page.keyboard.press("Backspace");
      await model.type("nomic-embed-text");
      await checked();
      await button("Save", "save-bar");
      await page.waitForSelector("[data-testid=diff-preview]");
      await button("Write config.toml", "save-bar");
      await page.waitForSelector("[data-testid=settings-notice]");
      await button("Sources", "settings");
    }
    const section = {
      retrieval: "Retrieval",
      threshold: "Retrieval",
      "slider-key": "Retrieval",
      "retrieval-end": "Retrieval",
      embedding: "Embedding",
      "embedding-end": "Embedding",
      remote: "Embedding",
      token: "Embedding",
      saved: "Embedding",
      "test-ok": "Embedding",
      "test-fail": "Embedding",
      model: "Embedding",
      memory: "Memory",
      documents: "Documents",
      updates: "Updates & backup",
      "updates-end": "Updates & backup",
      advanced: "Advanced",
      weights: "Advanced",
      raw: "Advanced",
      "raw-edit": "Advanced",
      "raw-error": "Advanced",
      "raw-copied": "Advanced",
    }[settings];
    if (section) {
      await button(section, "settings");
      await page.waitForNetworkIdle({ idleTime: 300 }).catch(() => {});
    }
    const fillIn = async (label, text) => {
      const field = await page.waitForSelector(`input[aria-label="${label}"]`);
      await field.click({ count: 3 });
      await page.keyboard.press("Backspace");
      await field.type(text);
    };
    if (settings === "threshold") {
      await fillIn("hook.threshold", "0.42");
      await checked();
    }
    if (settings === "slider-key") {
      await page.focus('input[aria-label="hook.threshold slider"]');
      await page.keyboard.press("ArrowRight");
      await checked();
    }
    if (settings === "remote" || settings === "saved") {
      await page.click("xpath/.//button[@role='radio' and normalize-space()='Remote endpoint']");
      if (settings === "saved") {
        await fillIn("endpoint.url", "http://127.0.0.1:9/v1");
        await fillIn("endpoint.model", "stub-model");
      }
      await button("Set token", "settings");
      await fillIn("embed.token", "sk-parity-secret");
      if (settings === "saved") {
        await button("Save endpoint", "settings");
        await page.waitForSelector("xpath/.//*[@role='status' and contains(., 'Endpoint saved')]");
      }
      await page.$eval("[data-testid=settings] .overflow-auto", (el) => (el.scrollTop = 0));
    }
    if (settings === "test-ok" || settings === "test-fail") {
      await button("Test connection", "settings");
      await page.waitForSelector("[data-testid=embed-test-result]", { timeout: 20_000 });
    }
    if (settings === "model") {
      await fillIn("embed.model", "nomic-embed-text");
      await checked();
    }
    if (settings === "weights") {
      await page.type('input[aria-label="hook.weights.transcript"]', "0.5");
      await checked();
    }
    if (settings.startsWith("raw")) {
      const bottom = () => page.$eval("[data-testid=settings] .overflow-auto", (el) => (el.scrollTop = el.scrollHeight));
      await bottom();
      const area = 'textarea[aria-label="raw config.toml"]';
      const typed = { "raw-edit": "[mcp]\nmax_tokens = 900\n", "raw-error": "oops = = 1\n" }[settings];
      if (typed) {
        await page.$eval(area, (el) => {
          el.focus();
          el.setSelectionRange(el.value.length, el.value.length);
        });
        await page.keyboard.type(typed);
      }
      if (settings === "raw-error") await button("Apply to form", "settings");
      if (settings === "raw-copied") {
        await page.evaluate(() => {
          Object.defineProperty(navigator, "clipboard", { value: { writeText: () => Promise.resolve() }, configurable: true });
        });
        await button("Copy", "settings");
        await page.waitForSelector("xpath/.//button[normalize-space()='Copied']");
      }
      await page.$eval(area, (el) => el.blur());
      await bottom();
    }
    if (["retrieval-end", "embedding-end", "updates-end"].includes(settings)) {
      await page.$eval("[data-testid=settings] .overflow-auto", (el) => (el.scrollTop = el.scrollHeight));
    }
    if (settings === "narrow") {
      await page.click('button[role=switch][aria-label="index_transcripts"]');
      await checked();
      await page.$eval("[data-testid=settings] .overflow-auto", (el) => (el.scrollTop = el.scrollHeight));
    }
    if (settings === "edit" || settings === "conflict") {
      await page.click('button[role=switch][aria-label="index_transcripts"]');
      await checked();
      if (settings === "conflict") appendFileSync(process.env.RHEBOK_CONFIG, "\n# edited by hand\n");
      await button("Save", "save-bar");
      await page.waitForSelector("[data-testid=diff-preview]");
      if (settings === "conflict") {
        await button("Write config.toml", "save-bar");
        await page.waitForSelector("[data-testid=conflict]");
      }
    }
    if (settings === "blocking" || settings === "ring") {
      await page.type('input[aria-label="index_transcripts_max_age_days"]', "abc");
      await checked();
    }
    if (settings === "ring") {
      await page.$eval("[data-testid=settings] .overflow-auto", (el) => (el.scrollTop = 0));
      await button("fix 1 problem to save", "save-bar");
      await page.waitForSelector("[data-path=index_transcripts_max_age_days].ring-2");
      await new Promise((done) => setTimeout(done, 100));
    } else {
      await page.waitForNetworkIdle({ idleTime: 300 }).catch(() => {});
    }
  }
  if (tab === "graph" && graph !== "error") {
    const nodes = () =>
      page.evaluate(() => {
        const canvas = document.querySelector(".force-graph-container canvas");
        const host = canvas.closest(".force-graph-container").parentElement;
        let fiber = host[Object.keys(host).find((k) => k.startsWith("__reactFiber"))];
        let top = fiber;
        while (top.return) top = top.return;
        if (top.stateNode.current !== top) fiber = fiber.alternate;
        while (fiber && !fiber.memoizedProps?.graphData) fiber = fiber.return;
        const t = canvas.__zoom;
        const r = canvas.getBoundingClientRect();
        return {
          k: t.k,
          left: r.left,
          top: r.top,
          width: r.width,
          height: r.height,
          nodes: fiber.memoizedProps.graphData.nodes.map((n) => ({ id: n.id, kind: n.kind, title: n.title, sx: t.x + t.k * n.x, sy: t.y + t.k * n.y })),
        };
      });
    let pan = null;
    const cool = () => new Promise((done) => setTimeout(done, 16_000));
    if (graph === "ring") {
      await openTab(page, "Search");
      const input = await page.waitForSelector("input");
      await input.type("how often do I feed the sourdough starter");
      await page.keyboard.press("Enter");
      await page.waitForNetworkIdle({ idleTime: 500 });
      await openTab(page, "Graph");
    }
    if (graph === "sessions") await page.click("xpath/.//button[starts-with(normalize-space(), 'Sessions')]");
    await cool();
    const kind = { document: "markdown", project: "project", memory: "memory" }[graph];
    if (kind) {
      const now = await nodes();
      const target = now.nodes.find((n) => n.kind === kind && n.title === "Flour") ?? now.nodes.find((n) => n.kind === kind);
      if (!target) throw new Error(`no ${kind} node to click`);
      await page.mouse.move(now.left + target.sx, now.top + target.sy);
      await new Promise((done) => setTimeout(done, 200));
      await page.mouse.click(now.left + target.sx, now.top + target.sy);
      await page.waitForNetworkIdle({ idleTime: 500 });
      await new Promise((done) => setTimeout(done, 300));
    }
    if (graph === "panned") {
      const now = await nodes();
      await page.mouse.move(now.left + now.width / 2, now.top + now.height / 2);
      await page.mouse.wheel({ deltaY: -1000 });
      await new Promise((done) => setTimeout(done, 400));
      const zoomed = await nodes();
      const corner = zoomed.nodes.reduce((a, b) => (b.sx + b.sy > a.sx + a.sy ? b : a));
      const dx = Math.round(zoomed.width - corner.sx);
      const dy = Math.round(zoomed.height - corner.sy);
      await page.mouse.move(now.left + 8, now.top + 8);
      await page.mouse.down();
      pan = { dx, dy };
      await page.mouse.move(now.left + 8 + dx, now.top + 8 + dy, { steps: 10 });
      await page.mouse.up();
      await new Promise((done) => setTimeout(done, 400));
    }
    if (graph === "hover") {
      const now = await nodes();
      const target = now.nodes.find((n) => n.kind === "markdown");
      await page.mouse.move(now.left + target.sx, now.top + target.sy);
      await new Promise((done) => setTimeout(done, 500));
    }
    if (graph === "zoom") {
      const now = await nodes();
      await page.mouse.move(now.left + now.width / 2, now.top + now.height / 2);
      await page.mouse.wheel({ deltaY: -200 });
      await new Promise((done) => setTimeout(done, 400));
    }
    if (graph !== "hover") {
      await page.mouse.move(0, 0);
      await new Promise((done) => setTimeout(done, 200));
    }
    const tooltip = await page.evaluate(() => {
      const tip = document.querySelector(".float-tooltip-kap");
      if (!tip || getComputedStyle(tip).display === "none") return null;
      const r = tip.getBoundingClientRect();
      return { left: r.left, top: r.top, width: r.width, height: r.height, text: tip.textContent };
    });
    if (process.env.RHEBOK_PARITY_NODES) writeFileSync(process.env.RHEBOK_PARITY_NODES, JSON.stringify({ ...(await nodes()), tooltip, pan }));
  }
  if (!(tab === "graph" && graph === "hover")) await page.mouse.move(0, 0);
  mkdirSync(dirname(resolve(out)), { recursive: true });
  await page.screenshot({ path: resolve(out) });
} finally {
  await browser.close();
}
