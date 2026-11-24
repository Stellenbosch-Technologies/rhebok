const collator = new Intl.Collator(undefined);
const base = new Intl.Collator(undefined, { sensitivity: "base" });
const sign = (a, b) => Math.sign(collator.compare(a, b));

const ascii = [];
for (let c = 0x20; c < 0x7f; c++) {
  const ch = String.fromCharCode(c);
  if (!/[A-Za-z0-9]/.test(ch)) ascii.push(ch);
}
const unicode = [
  ..."    ‐‑‒–—―−‘’‚‛“”„•·…«»‹›¡¿§¶©®°±×÷€£¥¢†‡′″¦¬™←→✓´",
];
const variable = [...new Set([...ascii, ...unicode])].sort((a, b) => collator.compare(a, b));
let primary = 0;
let tertiary = 0;
let previous = null;
const rows = [];
for (const ch of variable) {
  if (previous === null || base.compare(previous, ch) !== 0) {
    primary += 1;
    tertiary = 0;
  } else if (collator.compare(previous, ch) !== 0) {
    tertiary += 1;
  }
  rows.push(`    ('\\u{${ch.codePointAt(0).toString(16)}}', ${primary}, ${tertiary}),`);
  previous = ch;
}

const marks = [0x300, 0x301, 0x302, 0x303, 0x304, 0x306, 0x307, 0x308, 0x30a, 0x30b, 0x30c, 0x323, 0x327, 0x328, 0x331]
  .map((m) => "e" + String.fromCodePoint(m))
  .sort((a, b) => collator.compare(a, b))
  .map((s, i) => `    ('\\u{${s.codePointAt(1).toString(16)}}', ${i + 1}),`);

const titles = [
  "Ingest service runbook", "ingest service runbook", "Inbox", "_inbox", "(draft) plan", "my-note", "my_note",
  "my note", "Résumé", "Resume", "Retro", "Über", "über", "Zeitplan", "a—b", "a+b", "2024 goals", "10 things",
  "README", "readme", "Deploy checklist", "deploy-checklist", "Café notes", "Cafe notes", "“Quoted” idea",
  "'quoted' idea", "#tags", "@mentions", "Ångström", "Ölwechsel", "naïve plans", "Naive plans", "co-op", "coop",
  "élan", "Elan", "§ rules", "… ellipsis", "• bullets", "Zoë", "Zoe", "señor", "senor", "a.b", "a,b", "a b",
  "Q&A", "Q+A", "v1.2", "v1-2", "$ costs", "€ costs", "Sourdough starter", "sourdough-starter", "cote", "coté", "côte", "côté", "Pâté", "Pàté", "Cafè notes", "Pâtè",
];
const pairs = [];
for (let i = 0; i < titles.length; i++) {
  for (let j = i + 1; j < titles.length; j++) {
    pairs.push(`    (${JSON.stringify(titles[i])}, ${JSON.stringify(titles[j])}, ${sign(titles[i], titles[j])}),`);
  }
}

const locale = Intl.DateTimeFormat().resolvedOptions().locale;
console.log(`#[cfg(test)]
pub const NODE_LOCALE: &str = ${JSON.stringify(locale)};

pub const VARIABLE: [(char, u32, u32); ${rows.length}] = [
${rows.join("\n")}
];

pub const MARKS: [(char, u32); ${marks.length}] = [
${marks.join("\n")}
];

#[cfg(test)]
pub static PAIRS: [(&str, &str, i8); ${pairs.length}] = [
${pairs.join("\n")}
];`);
