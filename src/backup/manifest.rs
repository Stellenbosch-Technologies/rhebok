use crate::backup::crypto::Key;
use crate::config::{Config, EmbedConfig};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const MANIFEST_VERSION: u32 = 2;
pub const MANIFEST_AUTH_ALGO: &str = "hmac-sha256";
pub const ALLOW_UNAUTHENTICATED_FLAG: &str = "--allow-unauthenticated-manifest";
pub const ALLOW_UNENCRYPTED_FLAG: &str = "--allow-unencrypted-manifest";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManifestOverrides {
    pub untagged: bool,
    pub unencrypted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub hash: String,
    pub size: u64,
    pub mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub hash: String,
    pub size: u64,
    pub embed_model: String,
    pub dimensions: usize,
    pub chunk_tokens: usize,
    pub documents: usize,
    pub chunks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub hash: String,
    pub size: u64,
    pub embed_model: String,
    pub dimensions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestAuth {
    pub algo: String,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Encryption {
    pub algo: String,
    pub key_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub created_at: String,
    pub host: String,
    #[serde(alias = "br8n_version")]
    pub rhebok_version: String,
    #[serde(default)]
    pub encryption: Option<Encryption>,
    #[serde(default)]
    pub files: Vec<FileEntry>,
    #[serde(default)]
    pub index: Option<IndexEntry>,
    #[serde(default)]
    pub memory: Option<MemoryEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<ManifestAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestPointer {
    pub generation: String,
}

pub fn blob_key(key_id: Option<&str>, hash: &str) -> String {
    match key_id {
        Some(id) => format!("blobs/{id}/{hash}"),
        None => format!("blobs/{hash}"),
    }
}

pub fn index_key(key_id: Option<&str>, hash: &str) -> String {
    match key_id {
        Some(id) => format!("index/{id}/{hash}.tar.gz"),
        None => format!("index/{hash}.tar.gz"),
    }
}

pub fn memory_key(key_id: Option<&str>, hash: &str) -> String {
    match key_id {
        Some(id) => format!("memory/{id}/{hash}.tar.gz"),
        None => format!("memory/{hash}.tar.gz"),
    }
}

fn canonical_without_auth(m: &Manifest) -> Result<Vec<u8>> {
    let unsigned = Manifest {
        auth: None,
        ..m.clone()
    };
    Ok(serde_json::to_vec(&serde_json::to_value(&unsigned)?)?)
}

pub fn encode(m: &Manifest, key: Option<&Key>) -> Result<Vec<u8>> {
    let auth = match key {
        Some(k) => Some(ManifestAuth {
            algo: MANIFEST_AUTH_ALGO.into(),
            tag: k.manifest_tag(&canonical_without_auth(m)?),
        }),
        None => None,
    };
    Ok(serde_json::to_vec_pretty(&Manifest { auth, ..m.clone() })?)
}

pub fn decode(raw: &[u8]) -> Result<Manifest> {
    let doc: serde_json::Value = serde_json::from_slice(raw)?;
    Ok(serde_json::from_value(doc)?)
}

pub fn decode_authenticated(
    raw: &[u8],
    key: Option<&Key>,
    overrides: ManifestOverrides,
) -> Result<Manifest> {
    let mut doc: serde_json::Value =
        serde_json::from_slice(raw).context("could not parse the backup manifest")?;
    let auth = doc
        .as_object_mut()
        .context("the manifest is not a JSON object")?
        .remove("auth");
    let m: Manifest =
        serde_json::from_value(doc.clone()).context("could not parse the backup manifest")?;
    m.check_key(key)?;
    let Some(k) = key else {
        return Ok(m);
    };
    if m.encryption.is_none() {
        if overrides.unencrypted {
            return Ok(m);
        }
        bail!(
            "refusing this backup: its manifest says it is NOT encrypted, but a backup key is \
             configured, so neither the manifest nor anything it names can be authenticated. \
             Anyone who can write to the backup target can produce such a manifest and choose \
             every byte it restores, the memory store included. If you made this backup yourself \
             before turning on encryption, and nobody else can write to the target, re-run with \
             {ALLOW_UNENCRYPTED_FLAG}."
        );
    }
    let Some(auth) = auth else {
        if overrides.untagged {
            return Ok(m);
        }
        bail!(
            "refusing this backup: its manifest carries no authentication tag. Manifests written \
             by older versions of rhebok have none, and anyone who can write to the backup target \
             can strip it. If this is an older backup of your own and you trust the target, \
             re-run with {ALLOW_UNAUTHENTICATED_FLAG}; the next `rhebok backup` writes an \
             authenticated one."
        );
    };
    let auth: ManifestAuth =
        serde_json::from_value(auth).context("the manifest's authentication tag is malformed")?;
    if auth.algo != MANIFEST_AUTH_ALGO {
        bail!(
            "refusing this backup: its manifest is authenticated with {:?}, which this build does \
             not know (it expects {MANIFEST_AUTH_ALGO:?})",
            auth.algo
        );
    }
    if !k.manifest_tag_matches(&serde_json::to_vec(&doc)?, &auth.tag) {
        bail!(
            "refusing this backup: its manifest fails authentication under the configured key. \
             It was altered after it was written. Nothing was restored."
        );
    }
    Ok(m)
}

impl Manifest {
    pub fn new(key: Option<&Key>) -> Manifest {
        Manifest {
            version: MANIFEST_VERSION,
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            host: hostname(),
            rhebok_version: env!("CARGO_PKG_VERSION").to_string(),
            encryption: key.map(|k| Encryption {
                algo: "xchacha20poly1305".into(),
                key_id: k.id(),
            }),
            files: Vec::new(),
            index: None,
            memory: None,
            auth: None,
        }
    }

    pub fn check_generation(&self, generation: &str) -> Result<()> {
        let expected = self.generation_key();
        if generation != expected {
            bail!(
                "refusing this backup: {generation} holds a manifest written at {}, whose own name \
                 is {expected}. A manifest copied under another generation's name is how an old \
                 backup is passed off as a newer one. Nothing was restored.",
                self.created_at
            );
        }
        Ok(())
    }

    pub fn generation_key(&self) -> String {
        format!("manifest/{}.json", self.created_at.replace(':', "-"))
    }

    pub fn latest_key() -> &'static str {
        "manifest/latest.json"
    }

    pub fn check_key(&self, key: Option<&Key>) -> Result<()> {
        if self.version > MANIFEST_VERSION {
            return Err(anyhow!(
                "this backup was written by a newer `rhebok` (manifest version {}, this build understands {MANIFEST_VERSION}). Upgrade `rhebok` and try again.",
                self.version
            ));
        }
        match (&self.encryption, key) {
            (None, _) => Ok(()),
            (Some(_), None) => Err(anyhow!(
                "this backup is encrypted, but no key is configured. Point `key_file` at the key printed by `rhebok backup init`."
            )),
            (Some(e), Some(k)) if e.key_id != k.id() => Err(anyhow!(
                "this backup was made with a different key (backup key id {}, configured key id {}). Restoring would produce nothing but decryption failures.",
                e.key_id,
                k.id()
            )),
            (Some(_), Some(_)) => Ok(()),
        }
    }

    pub fn check_index_compat(&self, cfg: &Config) -> Result<()> {
        let Some(ix) = &self.index else {
            return Ok(());
        };
        let mismatch = |what: &str, backup: String, current: String| {
            anyhow!(
                "this backup's index was built with {what} {backup}, but the current config uses {current}. Embeddings from different settings are not comparable. Restore without `--index` and rebuild with `rhebok index --reindex`."
            )
        };
        if ix.embed_model != cfg.embed.model {
            return Err(mismatch(
                "embedding model",
                ix.embed_model.clone(),
                cfg.embed.model.clone(),
            ));
        }
        if ix.dimensions != cfg.embed.dimensions {
            return Err(mismatch(
                "dimensions",
                ix.dimensions.to_string(),
                cfg.embed.dimensions.to_string(),
            ));
        }
        if ix.chunk_tokens != cfg.embed.chunk_tokens {
            return Err(mismatch(
                "chunk size",
                ix.chunk_tokens.to_string(),
                cfg.embed.chunk_tokens.to_string(),
            ));
        }
        Ok(())
    }
}

impl MemoryEntry {
    pub fn incompatibility(&self, embed: &EmbedConfig) -> Option<String> {
        if self.embed_model == embed.model && self.dimensions == embed.dimensions {
            return None;
        }
        Some(format!(
            "the backup's memories were embedded with {} at {} dimensions, but the config in \
             effect uses {} at {}, so the memory store was NOT restored: this config could not \
             search or write it. To bring the memories back, set [embed] model and dimensions to \
             the backup's, run `rhebok restore --memories`, then put your settings back and run \
             `rhebok memory rebuild` to re-embed them.",
            self.embed_model, self.dimensions, embed.model, embed.dimensions
        ))
    }
}

fn hostname() -> String {
    if let Ok(h) = std::env::var("HOSTNAME") {
        if !h.is_empty() {
            return h;
        }
    }
    std::process::Command::new("uname")
        .arg("-n")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}
