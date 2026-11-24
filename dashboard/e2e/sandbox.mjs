import { join } from "node:path";
import { Sandbox } from "./rhebok.mjs";

const fresh = process.env.RHEBOK_SANDBOX_FRESH === "1";
const sandbox = await Sandbox.create({ indexed: !fresh });
try {
  if (!fresh) await sandbox.run("index");
  sandbox.binary = sandbox.placeBinary();
  await sandbox.startDashboard();
} catch (e) {
  sandbox.close();
  throw e;
}

const lines = {
  RHEBOK_DB: join(sandbox.dataDir, "db"),
  RHEBOK_CONFIG: sandbox.configPath,
  HOME: sandbox.home,
  CODEX_HOME: join(sandbox.home, ".codex"),
  RHEBOK_URL: sandbox.url,
  CORPUS: sandbox.corpus,
};
for (const [key, value] of Object.entries(lines)) console.log(`${key}=${value}`);
console.log("READY");

function stop() {
  sandbox.close();
  process.exit(0);
}
process.on("SIGINT", stop);
process.on("SIGTERM", stop);
setInterval(() => {}, 1 << 30);
