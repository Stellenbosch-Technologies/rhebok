import { join, resolve } from "node:path";
import { assert, clickButton, openTab, waitForText } from "../helpers.mjs";

export const fresh = true;
export const allowErrors = [/ERR_CONNECTION_REFUSED/, /ERR_EMPTY_RESPONSE/, /Failed to load resource/];

const SHOTS = resolve(new URL(".", import.meta.url).pathname, "../../../target/e2e-shots");

async function serverPid(url) {
  const r = await fetch(`${url}/api/version`);
  return (await r.json()).pid;
}

async function versionCard(page) {
  await page.waitForFunction(() => document.body.innerText.includes("Skip setup") || document.body.innerText.includes("Health"), { timeout: 20_000 });
  if (await page.$("[data-testid=onboarding]")) await clickButton(page, "Skip setup");
  await openTab(page, "Health");
  await page.waitForSelector("xpath/.//button[normalize-space()='Restart']", { timeout: 15_000 });
  return page.waitForSelector("[data-testid=version-card]");
}

export const tests = {
  "while an index runs, restart says what it costs and can be cancelled": async ({ page }) => {
    await page.setRequestInterception(true);
    page.on("request", (req) => {
      if (req.url().endsWith("/api/progress")) {
        req.respond({ status: 200, contentType: "application/json", body: JSON.stringify({ pct: 40, docs_done: 2, docs_total: 5, chunks: 9, eta_s: 30 }) });
      } else if (req.url().endsWith("/api/restart")) {
        req.respond({ status: 500, contentType: "application/json", body: JSON.stringify({ error: "the confirmation was skipped" }) });
      } else {
        req.continue();
      }
    });
    await page.reload({ waitUntil: "networkidle0" });
    const card = await versionCard(page);
    await waitForText(page, "an index is running");
    await clickButton(page, "Restart");
    await waitForText(page, "Restart stops it and starts it again");
    await card.screenshot({ path: join(SHOTS, "08-restart-confirm.png") });
    await clickButton(page, "Cancel");
    await page.waitForSelector("xpath/.//button[normalize-space()='Restart']");
    const text = await page.evaluate(() => document.body.innerText);
    assert(!text.includes("Restart stops it"), "the warning stayed after Cancel");
    assert(!text.includes("the confirmation was skipped"), "Restart was sent without the confirmation");
  },

  "restart brings the dashboard back on the same port and the page reloads onto it": async ({ page, sandbox }) => {
    const card = await versionCard(page);
    await card.screenshot({ path: join(SHOTS, "08-restart-card.png") });
    const before = await serverPid(sandbox.url);
    await page.evaluate(() => (window.__beforeRestart = true));
    let after = null;
    try {
      await Promise.all([
        page.waitForNavigation({ waitUntil: "networkidle0", timeout: 45_000 }),
        clickButton(page, "Restart"),
      ]);
      after = await serverPid(sandbox.url);
      assert(after !== before, `the same server (pid ${before}) still answers`);
      assert(!(await page.evaluate(() => window.__beforeRestart)), "the page did not reload");
      await versionCard(page);
    } finally {
      const running = after ?? (await serverPid(sandbox.url).catch(() => null));
      if (running && running !== before) {
        try {
          process.kill(running);
        } catch {}
      }
    }
  },
};
