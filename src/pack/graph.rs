use anyhow::{Context, Result};
use memmap2::Mmap;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use crate::expansion::{interleave, FOLLOWED_LINK_KINDS};

pub const GRAPH_FILE: &str = "pack.graph";

const MAGIC: &[u8; 8] = b"BRNGRPH1";
const HEADER: usize = 24;
const NONE: u32 = u32::MAX;

#[derive(Debug, Default)]
pub struct Source {
    pub chunks: Vec<(String, String, i64)>,
    pub documents: Vec<String>,
    pub next_chunk: Vec<(String, String)>,
    pub links: Vec<(String, String, String)>,
}

pub fn write(dir: &Path, row_chunk_ids: &[String], source: &Source) -> Result<()> {
    let row_of: HashMap<&str, u32> = row_chunk_ids
        .iter()
        .enumerate()
        .map(|(row, id)| (id.as_str(), row as u32))
        .collect();
    let documents: BTreeSet<&str> = source
        .documents
        .iter()
        .map(String::as_str)
        .chain(source.chunks.iter().map(|(_, doc, _)| doc.as_str()))
        .collect();
    let doc_of: HashMap<&str, u32> = documents
        .iter()
        .enumerate()
        .map(|(n, doc)| (*doc, n as u32))
        .collect();

    let rows = row_chunk_ids.len();
    let mut row_doc = vec![NONE; rows];
    let mut row_ord = vec![0u32; rows];
    let mut first_row = vec![NONE; documents.len()];
    for (chunk, doc, ord) in &source.chunks {
        let (Some(&row), Some(&d)) = (row_of.get(chunk.as_str()), doc_of.get(doc.as_str())) else {
            continue;
        };
        row_doc[row as usize] = d;
        row_ord[row as usize] = u32::try_from(*ord)
            .with_context(|| format!("pack graph: chunk {chunk} has ord {ord}"))?;
        if *ord == 0 {
            first_row[d as usize] = row;
        }
    }
    if let Some(row) = row_doc.iter().position(|d| *d == NONE) {
        anyhow::bail!(
            "pack graph: row {row} ({}) has no document",
            row_chunk_ids[row]
        );
    }

    let mut adjacent: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); rows];
    for (a, b) in &source.next_chunk {
        if let (Some(&a), Some(&b)) = (row_of.get(a.as_str()), row_of.get(b.as_str())) {
            adjacent[a as usize].insert(b);
            adjacent[b as usize].insert(a);
        }
    }
    let mut linked: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for (from, to, kind) in &source.links {
        if !FOLLOWED_LINK_KINDS.contains(&kind.as_str()) {
            continue;
        }
        if let (Some(&from), Some(&to)) = (doc_of.get(from.as_str()), doc_of.get(to.as_str())) {
            linked.entry(from).or_default().insert(to);
        }
    }
    let links: Vec<BTreeSet<u32>> = (0..documents.len() as u32)
        .map(|d| linked.remove(&d).unwrap_or_default())
        .collect();

    let adjacent_total: usize = adjacent.iter().map(BTreeSet::len).sum();
    let links_total: usize = links.iter().map(BTreeSet::len).sum();
    let mut words: Vec<u32> =
        Vec::with_capacity(3 * rows + 2 * documents.len() + adjacent_total + links_total + 2);
    words.extend(&row_doc);
    words.extend(&row_ord);
    words.extend(&first_row);
    push_lists(&mut words, &adjacent)?;
    push_lists(&mut words, &links)?;

    let p = dir.join(GRAPH_FILE);
    crate::pack::write_buffered(&p, |out| {
        use std::io::Write;
        out.write_all(MAGIC)?;
        for n in [rows, documents.len(), adjacent_total, links_total] {
            out.write_all(&(n as u32).to_le_bytes())?;
        }
        for w in &words {
            out.write_all(&w.to_le_bytes())?;
        }
        Ok(())
    })
}

fn push_lists(words: &mut Vec<u32>, lists: &[BTreeSet<u32>]) -> Result<()> {
    let mut offset = 0u32;
    words.push(0);
    for list in lists {
        offset = offset
            .checked_add(list.len() as u32)
            .context("pack graph: too many edges for a u32 offset")?;
        words.push(offset);
    }
    for list in lists {
        words.extend(list);
    }
    Ok(())
}

pub struct Reader {
    map: Mmap,
    rows: usize,
    docs: usize,
    adjacent: usize,
}

impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("graph::Reader")
            .field("rows", &self.rows)
            .field("docs", &self.docs)
            .finish()
    }
}

impl Reader {
    pub fn open(dir: &Path, rows: usize) -> Result<Option<Reader>> {
        let p = dir.join(GRAPH_FILE);
        let f = match std::fs::File::open(&p) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("open {}", p.display())),
        };
        let map = unsafe { Mmap::map(&f) }.context("mmap pack.graph")?;
        anyhow::ensure!(
            map.len() >= HEADER && &map[0..8] == MAGIC,
            "{} is not a pack.graph file",
            p.display()
        );
        let field = |i: usize| u32::from_le_bytes(map[8 + 4 * i..12 + 4 * i].try_into().unwrap());
        let (declared, docs, adjacent, links) = (
            field(0) as usize,
            field(1) as usize,
            field(2) as usize,
            field(3) as usize,
        );
        let words = 3 * declared + 2 * docs + adjacent + links + 2;
        anyhow::ensure!(
            declared == rows && map.len() == HEADER + 4 * words,
            "{} declares {declared} rows in {} bytes against the records' {rows} rows — \
             a stale graph would join one chunk's neighbours to another",
            p.display(),
            map.len()
        );
        Ok(Some(Reader {
            map,
            rows,
            docs,
            adjacent,
        }))
    }

    fn word(&self, i: usize) -> Result<u32> {
        let o = HEADER + 4 * i;
        let bytes = self
            .map
            .get(o..o + 4)
            .context("pack.graph read past its end")?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn doc_of(&self, row: u32) -> Result<u32> {
        anyhow::ensure!((row as usize) < self.rows, "pack.graph has no row {row}");
        self.word(row as usize)
    }

    fn ord_of(&self, row: u32) -> Result<u32> {
        self.word(self.rows + row as usize)
    }

    fn first_row(&self, doc: u32) -> Result<Option<u32>> {
        anyhow::ensure!(
            (doc as usize) < self.docs,
            "pack.graph has no document {doc}"
        );
        let row = self.word(2 * self.rows + doc as usize)?;
        Ok((row != NONE).then_some(row))
    }

    fn list(
        &self,
        offsets: usize,
        values: usize,
        count: usize,
        i: u32,
        limit: usize,
    ) -> Result<Vec<u32>> {
        anyhow::ensure!((i as usize) < count, "pack.graph list {i} is out of range");
        let (start, end) = (
            self.word(offsets + i as usize)? as usize,
            self.word(offsets + i as usize + 1)? as usize,
        );
        anyhow::ensure!(start <= end, "pack.graph offsets run backwards at {i}");
        (start..end)
            .map(|k| {
                let v = self.word(values + k)?;
                anyhow::ensure!(
                    (v as usize) < limit,
                    "pack.graph edge to {v} is out of range"
                );
                Ok(v)
            })
            .collect()
    }

    fn adjacent(&self, row: u32) -> Result<Vec<u32>> {
        let offsets = 2 * self.rows + self.docs;
        self.list(offsets, offsets + self.rows + 1, self.rows, row, self.rows)
    }

    fn links(&self, doc: u32) -> Result<Vec<u32>> {
        let offsets = 3 * self.rows + self.docs + 1 + self.adjacent;
        self.list(offsets, offsets + self.docs + 1, self.docs, doc, self.docs)
    }

    pub fn expand(&self, seeds: &[u32], hops: u8, max_neighbors: usize) -> Result<Vec<u32>> {
        if seeds.is_empty() || max_neighbors == 0 {
            return Ok(Vec::new());
        }
        let depth = hops.max(1);
        let seed_set: HashSet<u32> = seeds.iter().copied().collect();

        let reached = walk(&seed_set, depth, |row| self.adjacent(row))?;
        let mut adjacent = Vec::with_capacity(reached.len());
        for row in reached.into_iter().filter(|r| !seed_set.contains(r)) {
            adjacent.push((self.doc_of(row)?, self.ord_of(row)?, row));
        }
        adjacent.sort_unstable();
        let adjacent: Vec<u32> = adjacent.into_iter().map(|(_, _, row)| row).collect();

        let seed_docs = seeds
            .iter()
            .map(|&row| self.doc_of(row))
            .collect::<Result<HashSet<u32>>>()?;
        let mut linked_docs: Vec<u32> = walk(&seed_docs, depth, |doc| self.links(doc))?
            .into_iter()
            .collect();
        linked_docs.sort_unstable();
        let mut linked = Vec::new();
        for doc in linked_docs {
            if let Some(row) = self.first_row(doc)? {
                if !seed_set.contains(&row) {
                    linked.push(row);
                }
            }
        }

        Ok(interleave(&adjacent, &linked, max_neighbors))
    }
}

fn walk(
    start: &HashSet<u32>,
    depth: u8,
    next: impl Fn(u32) -> Result<Vec<u32>>,
) -> Result<HashSet<u32>> {
    let mut frontier = start.clone();
    let mut reached = HashSet::new();
    for _ in 0..depth {
        let mut step = HashSet::new();
        for node in &frontier {
            step.extend(next(*node)?);
        }
        reached.extend(step.iter().copied());
        frontier = step;
    }
    Ok(reached)
}
