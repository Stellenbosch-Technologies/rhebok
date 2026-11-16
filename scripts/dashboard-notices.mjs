import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";

const dashboard = process.argv[2];
if (!dashboard) {
  console.error("usage: node scripts/dashboard-notices.mjs <dashboard dir>");
  process.exit(2);
}

const lock = JSON.parse(readFileSync(join(dashboard, "package-lock.json"), "utf8"));
const shipped = Object.entries(lock.packages)
  .filter(([path, entry]) => path !== "" && !entry.dev)
  .map(([path]) => path)
  .sort();

const licenceFile = /^(licen[cs]e|copying|notice)(\.|-|$)/i;

function licenceFiles(root, prefix, depth) {
  const entries = readdirSync(join(root, prefix), { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  const here = entries
    .filter((entry) => entry.isFile() && licenceFile.test(entry.name))
    .map((entry) => join(prefix, entry.name));
  if (depth === 0) {
    return here;
  }
  const below = entries
    .filter((entry) => entry.isDirectory() && entry.name !== "node_modules")
    .flatMap((entry) => licenceFiles(root, join(prefix, entry.name), depth - 1));
  return [...here, ...below];
}

const missing = [];
const sections = [];

for (const path of shipped) {
  const dir = join(dashboard, path);
  if (!existsSync(join(dir, "package.json"))) {
    missing.push(`${path} is not installed; run npm ci in ${dashboard} first`);
    continue;
  }
  const manifest = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  const files = licenceFiles(dir, "", 2);
  if (files.length === 0 && !manifest.license) {
    missing.push(`${manifest.name} ${manifest.version} ships no licence file and declares no license`);
    continue;
  }
  const author = typeof manifest.author === "string" ? manifest.author : manifest.author?.name;
  const repository = typeof manifest.repository === "string" ? manifest.repository : manifest.repository?.url;
  sections.push(
    [
      `### ${manifest.name} ${manifest.version}`,
      "",
      `License: ${manifest.license ?? "not declared"}`,
      ...(author ? [`Author: ${author}`] : []),
      ...(repository ? [`Repository: ${repository}`] : []),
      "",
      ...(files.length === 0
        ? [`The package ships no licence file. Its declared license, ${manifest.license}, is reproduced in full in the Rust crates section above.`, ""]
        : files.flatMap((file) => [
            `${file}:`,
            "",
            "````text",
            readFileSync(join(dir, file), "utf8").trimEnd(),
            "````",
            "",
          ])),
    ].join("\n"),
  );
}

if (missing.length > 0) {
  console.error(missing.map((line) => `dashboard-notices: ${line}`).join("\n"));
  process.exit(1);
}

process.stdout.write(sections.join("\n"));
