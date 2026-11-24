import { pathToFileURL } from "node:url";
import { join } from "node:path";

const modules = process.env.RHEBOK_DASHBOARD_MODULES ?? new URL("../../../dashboard/node_modules", import.meta.url).pathname;
const { forceSimulation, forceLink, forceManyBody, forceCenter } = await import(
  pathToFileURL(join(modules, "d3-force-3d/src/index.js")).href
);

const [n, m, seed, ticks] = [50, 80, 3, 300];
let s = seed >>> 0;
const next = () => (s = (Math.imul(s, 1103515245) + 12345) >>> 0) / 4294967296;
const nodes = [...Array(n)].map((_, i) => ({ id: `n${i}` }));
const links = [...Array(m)].map(() => {
  const a = Math.floor(next() * n);
  const b = Math.floor(next() * n);
  return { source: `n${a}`, target: `n${b}` };
});

const sim = forceSimulation()
  .force("link", forceLink())
  .force("charge", forceManyBody())
  .force("center", forceCenter())
  .force("dagRadial", null)
  .stop();
sim.alphaDecay(0.0228).alphaTarget(0).velocityDecay(0.4);
sim.stop().alpha(1).nodes(nodes);
sim.force("link").id((d) => d.id).links(links);
for (let i = 0; i < ticks; i++) sim.tick();

const literal = (x) => {
  const text = String(x);
  return /[.e]/.test(text) ? text : `${text}.0`;
};
console.log(`pub const D3_SMALL_300: [(f64, f64); ${n}] = [`);
for (const node of nodes) console.log(`    (${literal(node.x)}, ${literal(node.y)}),`);
console.log("];");
