#[path = "../common/mod.rs"]
mod common;

mod agents;
mod audit;
#[cfg(feature = "backup")]
mod backup_cli;
#[cfg(feature = "backup")]
mod backup_config;
#[cfg(feature = "backup")]
mod backup_crypto;
#[cfg(feature = "backup")]
mod backup_drive;
#[cfg(feature = "backup")]
mod backup_manifest;
#[cfg(feature = "backup")]
mod backup_prune;
#[cfg(feature = "backup")]
mod backup_restore;
#[cfg(feature = "backup")]
mod backup_run;
#[cfg(feature = "backup")]
mod backup_s3;
#[cfg(feature = "backup")]
mod backup_snapshot;
mod chunk;
mod cli;
mod config_edit;
mod context_cli;
mod digests;
mod embed;
mod embed_openai;
mod enrich;
mod env_file;
mod frontmatter_reindex;
mod hook_contract;
mod imports;
mod incremental_index;
mod index_graph;
mod install_sh;
mod loaders_codex;
mod loaders_markdown;
mod loaders_transcript;
mod loaders_web;
mod mcp;
mod memory;
mod memory_distill;
mod migrate;
mod pack;
mod plugin_launcher;
mod plugin_manifest;
mod prehook;
mod probe_chunk;
mod probe_insert;
mod project;
mod project_memories;
mod reindex_safety;
mod remote_misconfigured;
mod rerank;
mod retrieve_graph;
mod retrieve_memory;
mod retrieve_primitives;
mod retrieve_profile;
mod scope;
mod setup_claude;
mod setup_homebrew;
mod setup_install;
mod setup_ollama;
mod setup_plugin;
mod slices;
mod store;
mod surface_weights;
mod update;
mod usage;
