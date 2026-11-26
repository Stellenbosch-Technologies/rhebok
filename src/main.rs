use anyhow::Result;
use clap::{Parser, Subcommand};
use rhebok::budget::{escape_reserved_tags, one_line, quote_lines, CONTEXT_PREAMBLE};
use rhebok::config::{Config, Profile, Surface};
use rhebok::index::{discover, reindex_swap_with, Indexer, RebuildMode};
use rhebok::retrieve::Retriever;
use rhebok::store::{query::Hit, StatusSnapshot, Store};

#[derive(Parser)]
#[command(name = "rhebok", version, about = "Local RAG over your knowledge base")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the native desktop app
    App {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Index every configured source
    Index {
        /// Delete the existing index and rebuild from scratch
        #[arg(long, conflicts_with = "compact")]
        reindex: bool,
        /// Reclaim space lbug cannot: rebuild the index from its own rows,
        /// reusing every stored embedding. No re-embedding, no Ollama traffic.
        #[arg(long)]
        compact: bool,
        /// Publish immediately with no embeddings at all: BM25 keyword
        /// search works the moment this returns, vector search stays empty
        /// until `--backfill` runs. Phase 1 of an asynchronous index.
        #[arg(long, conflicts_with_all = ["compact", "backfill"])]
        no_embed: bool,
        /// Embed the backlog `--no-embed` (or a previous, interrupted
        /// `--backfill`) left behind, then republish the pack. Runs in short
        /// `IndexLock` slices and yields to prompts between batches — never
        /// holds the store open for the whole run. Phase 2 of an
        /// asynchronous index.
        #[arg(long, conflicts_with_all = ["compact", "reindex", "no_embed"])]
        backfill: bool,
    },
    /// Search the index
    Search {
        query: Vec<String>,
        #[arg(long)]
        quality: Option<u8>,
        #[arg(long)]
        json: bool,
    },
    /// Print the context the prompt hook would inject for the prompt on stdin
    Context {
        #[arg(long)]
        json: bool,
        /// Answer as the prehook mod's fallback: search only when `[prehook] injector` is `mod`
        #[arg(long)]
        prehook: bool,
        /// The Claude Code session id the prehook is injecting for
        #[arg(long, requires = "prehook")]
        session: Option<String>,
        /// The directory the prompt was written in, which selects the project (default: this one)
        #[arg(long)]
        cwd: Option<std::path::PathBuf>,
    },
    /// Show index size and configuration
    Status,
    /// Check the embedding endpoint and the index agree
    Doctor,
    /// Add a single URL or file to the index
    Add { target: String },
    /// Measure recall and latency at every quality tier
    Bench {
        #[arg(long)]
        no_graph: bool,
        #[arg(
            long,
            conflicts_with = "synthetic",
            help = "Keep the sessions that quote a golden query instead of leaving them out"
        )]
        keep_footprint: bool,
        /// Measure one project: only its golden cases, searched inside its scope
        #[arg(long, conflicts_with = "synthetic")]
        project: Option<String>,
        #[arg(
            long,
            value_name = "CHUNKS",
            conflicts_with = "no_graph",
            help = "Build a synthetic pack of this many chunks and time it, without Ollama"
        )]
        synthetic: Option<usize>,
        #[arg(long, default_value_t = 0, requires = "synthetic")]
        seed: u64,
        #[arg(
            long,
            value_name = "DIR",
            requires = "synthetic",
            conflicts_with = "reuse",
            help = "Leave the synthetic index here, laid out as a live index directory"
        )]
        out: Option<std::path::PathBuf>,
        #[arg(
            long,
            value_name = "DIR",
            requires = "synthetic",
            conflicts_with = "out",
            help = "Skip the build and query an index a matching --out already wrote at the same chunks and seed"
        )]
        reuse: Option<std::path::PathBuf>,
        #[arg(long, requires = "synthetic")]
        json: bool,
    },
    /// Read, check and change config.toml without losing its comments
    Config {
        #[command(subcommand)]
        what: ConfigCmd,
    },
    /// Build and check the golden set `rhebok bench` measures against
    Golden {
        #[command(subcommand)]
        what: GoldenCmd,
    },
    /// Remember, list and forget lessons, facts and episodes
    Memory {
        #[command(subcommand)]
        what: MemoryCmd,
    },
    /// List projects, show which one a directory belongs to, or trust a repo's project file
    Project {
        #[command(subcommand)]
        what: ProjectCmd,
    },
    /// Read the hook's own injection ledger: what it really put in prompts.
    ///
    /// `rhebok bench` measures retrieval against a golden set of vault notes.
    /// This measures what the hook ACTUALLY injected, which is a different
    /// question and, as of this writing, a very different answer.
    AuditInjections {
        /// Claude Code's transcript root. Defaults to ~/.claude/projects.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// Run the MCP server on stdio
    Mcp,
    /// Internal: hook entrypoints
    Hook {
        #[arg(value_parser = ["prompt", "session-start", "load", "mod-start"])]
        which: String,
        /// Which agent's hook envelope to read and write
        #[arg(long, value_parser = ["claude-code", "codex", "gemini"], default_value = "claude-code")]
        agent: String,
    },
    /// Serve the local web dashboard (graph, search, health)
    Dashboard {
        /// Port to bind (default: OS-assigned)
        #[arg(long)]
        port: Option<u16>,
        /// Do not open the browser
        #[arg(long)]
        no_open: bool,
    },
    /// List the coding agents rhebok can connect to, and whether each is connected
    Agents {
        /// Print the listing as JSON
        #[arg(long)]
        json: bool,
    },
    /// Connect rhebok to one or more agents: claude-code, codex, claude-desktop, cursor, gemini
    Connect {
        agents: Vec<String>,
        /// Codex only: also add a marked guidance block to Codex's global AGENTS.md
        #[arg(long)]
        instructions: bool,
        /// Connect every agent found on this machine
        #[arg(long, conflicts_with = "agents")]
        all_detected: bool,
        /// Print a config snippet instead of writing anything: an agent id, `mcp-json` or `codex-toml`
        #[arg(long, value_name = "TARGET", conflicts_with_all = ["agents", "all_detected", "instructions"])]
        print: Option<String>,
    },
    /// Remove rhebok from one or more agents' configuration
    Disconnect {
        #[arg(required = true)]
        agents: Vec<String>,
    },
    /// Install rhebok into its own directory, register it with Claude Code, and offer other agents
    Install {
        /// Answer yes to every prompt (the model pull)
        #[arg(long)]
        yes: bool,
        /// Print failures only
        #[arg(long)]
        quiet: bool,
        /// Embedding model to use: qwen3 (the default), gemma, an Ollama model name, a model
        /// page on ollama.com or huggingface.co, or the URL of an OpenAI-compatible embedding
        /// server. Asked on a first interactive install
        #[arg(long, value_name = "MODEL")]
        embed_model: Option<String>,
    },
    /// Remove rhebok: the binary, the plugin, the PATH link, and every agent connection
    Uninstall {
        /// Also remove the index and the config
        #[arg(long)]
        purge: bool,
        /// Do not ask before purging
        #[arg(long)]
        yes: bool,
    },
    /// Stop the running dashboard and any running index, then start them again on the installed binary
    Restart,
    /// Fetch the latest release, verify it, and let it install itself
    Update {
        /// Only report whether a newer version exists
        #[arg(long)]
        check: bool,
        /// Accepted for scripts; the update never prompts
        #[arg(long)]
        yes: bool,
        /// Print failures only
        #[arg(long)]
        quiet: bool,
    },
    /// Back up the config, the golden set, the memory store, and the index; no subcommand runs one backup
    #[cfg(feature = "backup")]
    Backup {
        #[command(subcommand)]
        action: Option<BackupAction>,
    },
    /// Restore the config, the golden set, the memory store, and optionally the index, from a backup
    #[cfg(feature = "backup")]
    Restore {
        /// A generation key, e.g. `manifest/2026-08-25T03-00-00.000Z.json`; defaults to the newest
        #[arg(long)]
        generation: Option<String>,
        /// Also restore the index; refused if it was built with a different model, width, or chunk size
        #[arg(long)]
        index: bool,
        /// Print what would be written and write nothing
        #[arg(long)]
        dry_run: bool,
        /// Overwrite config.toml and golden.toml when they already exist locally
        #[arg(long)]
        force: bool,
        /// Replace a memory store that already exists locally; the local one is kept beside it as `db.before-restore-<time>`
        #[arg(long)]
        memories: bool,
        /// Accept an encrypted manifest that carries no authentication tag, as every rhebok before manifest authentication wrote
        #[arg(long)]
        allow_unauthenticated_manifest: bool,
        /// Accept a manifest that says it is unencrypted although a backup key is configured; anyone who can write to the target can forge one
        #[arg(long)]
        allow_unencrypted_manifest: bool,
    },
}

#[cfg(feature = "backup")]
#[derive(Subcommand)]
enum BackupAction {
    /// Generate the encryption key
    Init {
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Validate credentials and reachability the way cron will see them
    Check,
    /// Authorize Google Drive once, interactively, and create the backup folder
    Auth {
        #[arg(value_parser = ["drive"])]
        provider: String,
    },
    /// Show the last success, the targets, and how many generations each holds
    Status,
    /// Install or remove the crontab entry
    Schedule {
        /// A cron schedule
        #[arg(long, default_value = "0 13 * * *")]
        at: String,
        /// Remove the entry instead of installing it
        #[arg(long)]
        uninstall: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Report every problem in config.toml; exit non-zero if there are any
    Check,
    /// Print the effective value of a dotted key, e.g. `hook.threshold`
    Get { key: String },
    /// Set a dotted key; the value is read as TOML, or as a string if it is not TOML
    Set { key: String, value: String },
    /// Remove a dotted key from config.toml so its default applies again
    Unset { key: String },
    /// Print the path of config.toml
    Path,
}

#[derive(Subcommand)]
enum GoldenCmd {
    /// Write a starter golden set, seeded with real documents from your index
    Init {
        /// Overwrite an existing golden set. It may be your only copy.
        #[arg(long)]
        force: bool,
    },
    /// Report every problem in the golden set; exit non-zero if any are errors
    Check,
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// Every project in config.toml, with its roots
    List,
    /// Which project a directory belongs to, and why
    Show {
        /// The directory to resolve (default: this one)
        #[arg(long)]
        cwd: Option<std::path::PathBuf>,
    },
    /// Trust a repository's .rhebok/project.toml as it is now; any later edit untrusts it
    Trust {
        /// The repository directory, or the project file itself
        path: std::path::PathBuf,
    },
    /// Every project and slice a document is in or out of, and the rule that decided it
    Explain {
        /// A URI from search results, or a file path
        target: String,
    },
    /// Rebuild the index's project scopes from the current rules, without reindexing
    Rescope,
    /// Write a project or `<project>:<slice>` as a standalone, read-only pack others can mount
    Export {
        /// The project, or `<project>:<slice>`
        name: String,
        /// An empty directory to write the slice into
        #[arg(long)]
        out: std::path::PathBuf,
        /// Also export session transcripts (left out by default: they hold whatever was said)
        #[arg(long)]
        include_transcripts: bool,
        /// List what would be exported and write nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Make an exported slice importable by name; it is searched read-only, never changed
    Mount {
        /// The exported slice's directory
        dir: std::path::PathBuf,
        /// The name projects import it by (default: the name it was exported under)
        #[arg(long = "as")]
        name: Option<String>,
    },
    /// Forget a mount; the slice's files are left where they are
    Unmount { name: String },
    /// Print the instructions for a Claude Desktop Chat Project that should draw on this project
    Instructions { name: String },
}

#[derive(Subcommand)]
enum MemoryCmd {
    /// Save a memory. Text is the remaining arguments.
    Add {
        /// lesson, fact or episode
        #[arg(long, default_value = "lesson")]
        kind: String,
        /// Scope to this project directory (default: the current directory)
        #[arg(long, conflicts_with = "global")]
        project: Option<std::path::PathBuf>,
        /// Apply everywhere, not only in the current project
        #[arg(long)]
        global: bool,
        #[arg(long)]
        title: Option<String>,
        text: Vec<String>,
    },
    /// List memories, newest first
    List {
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        project: Option<std::path::PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Delete a memory by id (a unique prefix is enough)
    Forget { id: String },
    /// Make a project memory global, so it applies in every project
    Promote { id: String },
    /// Distill episodes from finished session transcripts
    Distill {
        /// Drain every pending session, not just one
        #[arg(long)]
        all: bool,
        /// Distill exactly this transcript
        #[arg(long)]
        session: Option<std::path::PathBuf>,
    },
    /// Re-embed every memory with the configured model and republish
    Rebuild,
    /// Write every memory as JSON lines
    Export {
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Read JSON lines written by export and save each as a memory
    Import { file: std::path::PathBuf },
}

/// Render the progress file written by a running index, if one is active.
///
/// Returns `None` when no run is in progress, and also when the file is stale:
/// a killed indexer cannot clean up after itself, so a progress file whose
/// process is gone must not be reported as live work.
fn read_progress(path: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let num = |k: &str| -> f64 {
        raw.split(&format!("\"{k}\":"))
            .nth(1)
            .and_then(|s| {
                s.split(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                    .find(|t| !t.is_empty())
            })
            .and_then(|t| t.parse().ok())
            .unwrap_or(0.0)
    };
    let pid = num("pid") as u32;
    if pid != 0 && !rhebok::index::pid_is_alive(pid) {
        let _ = std::fs::remove_file(path);
        return None;
    }
    let mins = |s: f64| format!("{}m{:02}s", (s as u64) / 60, (s as u64) % 60);
    Some(format!(
        "{:.1}%  {}/{} docs  {} chunks  {} elapsed  {} left",
        num("pct"),
        num("docs_done") as u64,
        num("docs_total") as u64,
        num("chunks") as u64,
        mins(num("elapsed_s")),
        mins(num("eta_s")),
    ))
}

fn confirm_on_stdin(question: &str, default_yes: bool) -> bool {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprint!(
        "? {question} {} ",
        if default_yes { "[Y/n]" } else { "[y/N]" }
    );
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    rhebok::setup::answer_is_yes(&line, default_yes)
}

fn choose_embed_on_stdin() -> rhebok::embed::source::ModelSource {
    use rhebok::embed::choice::CHOICES;
    use rhebok::embed::source::{parse, ModelSource};
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        return ModelSource::Preset(&CHOICES[0]);
    }
    eprintln!("? which embedding model should rhebok use?");
    for (i, c) in CHOICES.iter().enumerate() {
        eprintln!("  {}) {} ({}): {}", i + 1, c.label, c.size, c.summary);
    }
    eprintln!(
        "  or paste an Ollama model name, a model page on ollama.com or huggingface.co, or the \
         URL of an OpenAI-compatible embedding server such as http://localhost:1234"
    );
    loop {
        eprint!("  choose [1] ");
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => return ModelSource::Preset(&CHOICES[0]),
            Ok(_) => {}
        }
        match parse(&line) {
            Ok(source) => return source,
            Err(e) => eprintln!("  {e:#}"),
        }
    }
}

fn read_secret_on_stdin(question: &str) -> Option<String> {
    eprint!("? {question} ");
    let fd = libc::STDIN_FILENO;
    let mut term = std::mem::MaybeUninit::<libc::termios>::uninit();
    let original = (unsafe { libc::tcgetattr(fd, term.as_mut_ptr()) } == 0)
        .then(|| unsafe { term.assume_init() });
    if let Some(original) = original {
        let mut hidden = original;
        hidden.c_lflag &= !libc::ECHO;
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &hidden) };
    }
    let mut line = String::new();
    let read = std::io::stdin().read_line(&mut line);
    if let Some(original) = original {
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &original) };
    }
    eprintln!();
    read.ok()?;
    Some(line.trim().to_string()).filter(|t| !t.is_empty())
}

struct ChosenEndpoint {
    url: String,
    model: String,
    dimensions: usize,
    token: Option<String>,
}

fn connect_endpoint(url: &str, interactive: bool) -> Result<ChosenEndpoint> {
    use rhebok::embed::probe;
    use rhebok::env_file::{MODEL_VAR, TOKEN_VAR};
    let from_env = |key: &str| std::env::var(key).ok().filter(|v| !v.trim().is_empty());
    let token = if interactive {
        read_secret_on_stdin(&format!(
            "token for {url} (press Enter if it needs none; it is not shown)"
        ))
    } else {
        from_env(TOKEN_VAR)
    };
    let listed = probe::endpoint_models(url, token.as_deref());
    let model = if interactive {
        let names = listed.as_ref().cloned().unwrap_or_default();
        match &listed {
            Ok(names) if !names.is_empty() => {
                eprintln!("? which model at {url} should rhebok embed with?");
                for (i, m) in names.iter().enumerate() {
                    eprintln!("  {}) {m}", i + 1);
                }
                eprint!("  choose{} ", if names.len() == 1 { " [1]" } else { "" });
            }
            Ok(_) => eprint!("? {url} lists no models; type the model name: "),
            Err(e) => {
                eprint!("? could not list the models at {url} ({e:#}); type the model name: ")
            }
        }
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        rhebok::setup::model::pick_model(&names, &line)
            .ok_or_else(|| anyhow::anyhow!("no model was chosen for {url}"))?
    } else {
        match (from_env(MODEL_VAR), &listed) {
            (Some(m), _) => m,
            (None, Ok(names)) if names.len() == 1 => names[0].clone(),
            (None, Ok(names)) => anyhow::bail!(
                "{url} serves {} models; set {MODEL_VAR} to the one to embed with: {}",
                names.len(),
                names.join(", ")
            ),
            (None, Err(e)) => anyhow::bail!("{e:#}; set {MODEL_VAR} to the model to embed with"),
        }
    };
    let dimensions = probe::endpoint_dimensions(url, &model, token.as_deref())?;
    rhebok::setup::model::write_endpoint(&Config::config_path(), url, &model, token.as_deref())?;
    Ok(ChosenEndpoint {
        url: url.to_string(),
        model,
        dimensions,
        token,
    })
}

fn run_agents(json: bool) -> Result<()> {
    use rhebok::setup::agents;
    let env = agents::AgentEnv::from_env();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&agents::api::list(&env))?
        );
        return Ok(());
    }
    println!(
        "{:<16} {:<22} {:<16} CAPABILITIES",
        "AGENT", "DETECTED", "STATUS"
    );
    let mut reasons = Vec::new();
    for agent in agents::all() {
        let detected = agent.detect(&env);
        let status = agent.status(&env);
        let seen = match (&detected.installed, &detected.version) {
            (true, Some(v)) => format!("yes ({})", v.split_whitespace().next().unwrap_or(v)),
            (true, None) => "yes".to_string(),
            (false, _) => "no".to_string(),
        };
        println!(
            "{:<16} {:<22} {:<16} {}",
            agent.id(),
            seen,
            status.state().replace('_', " "),
            agent.capabilities().names().join(", ")
        );
        if let Some(r) = status.reason() {
            reasons.push(format!("  {}: {r}", agent.id()));
        }
    }
    for r in reasons {
        println!("{r}");
    }
    Ok(())
}

fn run_connect(
    ids: Vec<String>,
    instructions: bool,
    all_detected: bool,
    print: Option<String>,
) -> Result<()> {
    use rhebok::setup::agents;
    let env = agents::AgentEnv::from_env();
    if let Some(target) = print {
        let text = match target.as_str() {
            "mcp-json" => agents::mcp_json_snippet(&env),
            "codex-toml" => agents::codex_toml_snippet(&env),
            id => agents::find(id)?.snippet(&env).unwrap_or_default(),
        };
        print!("{text}");
        return Ok(());
    }
    let chosen: Vec<Box<dyn agents::Agent>> = if all_detected {
        agents::all()
            .into_iter()
            .filter(|a| a.detect(&env).installed)
            .collect()
    } else if ids.is_empty() {
        anyhow::bail!(
            "name at least one agent ({}), or pass --all-detected",
            agents::ids().join(", ")
        );
    } else {
        ids.iter()
            .map(|id| agents::find(id))
            .collect::<Result<_>>()?
    };
    let mut failed = 0;
    for agent in &chosen {
        let opts = agents::ConnectOptions {
            instructions: instructions && agent.capabilities().instructions,
        };
        if instructions && !agent.capabilities().instructions && !all_detected {
            eprintln!(
                "! {}: --instructions applies to codex only; ignored",
                agent.id()
            );
        }
        match agents::connect(agent.as_ref(), &env, &opts) {
            Ok(change) => println!("{}", agents::describe(agent.as_ref(), &change)),
            Err(e) => {
                failed += 1;
                eprintln!("! {}: {e:#}", agent.id());
            }
        }
    }
    if chosen.is_empty() {
        println!("no agents detected");
    }
    if failed > 0 {
        anyhow::bail!("{failed} of {} agents could not be connected", chosen.len());
    }
    Ok(())
}

fn run_disconnect(ids: Vec<String>) -> Result<()> {
    use rhebok::setup::agents;
    let env = agents::AgentEnv::from_env();
    let chosen = ids
        .iter()
        .map(|id| agents::find(id))
        .collect::<Result<Vec<_>>>()?;
    let mut failed = 0;
    for agent in &chosen {
        match agent.disconnect(&env) {
            Ok(change) if change.is_empty() => {
                println!("{}: not connected, nothing changed", agent.id())
            }
            Ok(change) => {
                let mut parts: Vec<String> = change
                    .files
                    .iter()
                    .map(|f| format!("updated {}", f.display()))
                    .collect();
                parts.extend(change.notes.iter().cloned());
                println!("{}: disconnected ({})", agent.id(), parts.join("; "));
            }
            Err(e) => {
                failed += 1;
                eprintln!("! {}: {e:#}", agent.id());
            }
        }
    }
    if failed > 0 {
        anyhow::bail!(
            "{failed} of {} agents could not be disconnected",
            chosen.len()
        );
    }
    Ok(())
}

pub fn build_indexer(cfg: &Config) -> Result<Indexer> {
    let store = Store::open(&Config::db_path(), cfg.embed.dimensions)?;
    Ok(Indexer::new(
        store,
        rhebok::embed::for_config(&cfg.embed)?,
        cfg.clone(),
    ))
}

/// `rhebok search` is ungated, so weights only reorder here and the global
/// `[weights]` table — the one whose whole job is ordering — is the right one.
/// A surface's overrides exist to move its GATE; borrowing them here would make
/// the CLI rank differently from the hook for no reason a user asked for.
///
/// `profile` is the tier this call is actually about to search at — `Cmd::Search`
/// computes it (`--quality`, or the MCP surface's default) before calling this —
/// not a fixed surface default, so the store-vs-pack decision below is made
/// against the tier that will really run, the same distinction `retrieve_for_profile`
/// draws against `retrieve_for` in `lib.rs`.
pub fn build_retriever(cfg: &Config, profile: &Profile) -> Result<Retriever> {
    let db = Config::db_path();
    let embedder = rhebok::embed::for_config(&cfg.embed)?;
    let model_id = embedder.model_id();
    // Same lookup `retrieve_for` uses for every other surface (the hook, the
    // dashboard, the MCP tool, `rhebok bench`): a missing pack degrades to the
    // store, an invalid one refuses. Without this, `rhebok search` — the tool
    // reached for to debug "why didn't the hook inject X" — queried a
    // different backend than the hook it is meant to be debugging.
    let pack = rhebok::pack::open_pack_beside(&db, &model_id, cfg.embed.dimensions)?;
    // RE-ENABLED, reversing the MEASURED HOLD this comment used to describe.
    //
    // On the 100-case golden set, same corpus, quiet machine, bench repeatable
    // to 0.01, the pack loses recall at every tier where it matters:
    //
    //     tier         store   pack    latency
    //     instant      0.52    0.42    -48%
    //     fast (hook)  0.83    0.76    -17%
    //     balanced     0.85    0.80    -17%
    //     thorough     0.86    0.84    -22%
    //     exhaustive   0.87    0.87    -24%
    //
    // Seven cases in a hundred at the hook's tier. That table has not been
    // re-measured since; task 7 re-benches it and returns the decision.
    // Re-enabling here is necessary for that measurement to be possible, not
    // a verdict that the trade is now good — see `retrieve_for` in `lib.rs`
    // for the fuller account of why stage 1b changes that verdict.
    //
    // `profile` IS the single profile `--quality` is about to search at (the
    // caller computed it before calling this), so the same store-vs-pack
    // decision `retrieve_for_profile` makes for the hook and the dashboard
    // applies here too: a tier that needs no graph expansion reads the pack
    // alone. `rhebok search`'s weights are the ungated global table (see the
    // doc comment above), not a surface's override, so authority is read from
    // `cfg.weights` directly rather than through `weights_for`.
    //
    // Authority no longer forces the store open: `pack.links` carries the
    // inbound counts (see `retrieve_for_profile` in `lib.rs` for the measured
    // cost it used to add). With NO pack the counts are still store-only, so
    // the store must still open for it — `&& pack.is_none()`, not a dropped
    // clause.
    let needs_store = profile.graph.is_some() || (cfg.weights.authority > 0.0 && pack.is_none());

    let retriever = if !needs_store {
        match pack {
            // The store-free path this decision exists for: no `Database::new`,
            // no `LOAD EXTENSION`, nothing but the mmap'd pack.
            Some(pk) => Retriever::packed(pk, embedder, cfg.embed.ollama_url.clone()),
            // No pack validated (an index built before packs existed, or one
            // that failed validation above would already have returned `Err`)
            // — vector/bm25/measure still need somewhere to read from.
            None => {
                let store = Store::open_existing(&db, cfg.embed.dimensions)?;
                Retriever::new(store, embedder, cfg.embed.ollama_url.clone())
            }
        }
    } else {
        // Graph expansion, or authority weighting with no pack to read the
        // counts from: the store opens regardless. The pack is still attached
        // when present — vector/bm25/measure read it either way.
        let store = Store::open_existing(&db, cfg.embed.dimensions)?;
        Retriever::new(store, embedder, cfg.embed.ollama_url.clone()).with_pack(pack)
    };

    let memory = rhebok::memory::open_pack(cfg, &model_id);
    Ok(retriever
        .with_memory(memory, cfg.memory.clone())
        .with_weights(cfg.weights.clone()))
}

/// One summary line naming titles/filename stems shared by more than one
/// document — those wikilinks resolve to nothing (ambiguous fails closed, same
/// as unresolvable). Printed once after the run, not per document: ambiguous
/// keys are bounded by duplicate titles, not corpus size, so a per-document
/// `eprintln!` would be silent 99% of the time and spammy the other 1%.
fn print_ambiguous_titles(idx: &Indexer) {
    if let Ok(ambiguous) = idx.ambiguous_titles() {
        if !ambiguous.is_empty() {
            println!(
                "ambiguous wikilink targets (unresolved): {}",
                ambiguous.join(", ")
            );
        }
    }
}

fn render_hit(h: &Hit, styled: bool) -> String {
    let title = one_line(&h.title);
    let mut entry = if styled {
        format!("\n\x1b[1m{title}\x1b[0m  ({:.3})\n", h.relevance)
    } else {
        format!("\n{title}  ({:.3})\n", h.relevance)
    };
    for line in [&h.heading_path, &h.uri] {
        if line.is_empty() {
            continue;
        }
        entry.push_str(&if styled {
            format!("  \x1b[2m{}\x1b[0m\n", one_line(line))
        } else {
            format!("  {}\n", one_line(line))
        });
    }
    let excerpt: String = h.text.chars().take(300).collect();
    entry.push_str(&quote_lines(&excerpt));
    entry
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load();

    // Before the match, because every arm below can reach a loader and some of
    // them spawn threads: `Cmd::Mcp` builds a multi-thread runtime and
    // `Cmd::Hook` spawns the embedder warm-up. `std::env::set_var` is only
    // sound while this process is single-threaded, and this is the last point
    // at which it is. See `resolve_ocr_libraries`.
    rhebok::loaders::pdf::resolve_ocr_libraries(&cfg.pdf);

    match cli.cmd {
        Cmd::Index {
            reindex,
            compact,
            no_embed,
            backfill,
        } => {
            let outcome = run_index(&cfg, reindex, compact, no_embed, backfill);
            rhebok::dashboard::core::index::record_own_outcome(
                &Config::db_path(),
                if outcome.is_ok() { 0 } else { 1 },
            );
            outcome?;
        }

        Cmd::Search {
            query,
            quality,
            json,
        } => {
            let q = query.join(" ");
            let profile = quality
                .map(Profile::tier)
                .unwrap_or_else(|| cfg.profile_for(Surface::Mcp));
            // A search that cannot reach the index prints nothing on stdout —
            // stdout is the result channel and `--json` must stay parseable —
            // but it SAYS SO on stderr and exits 0.
            //
            // This swallowed every error silently, which made the one command a
            // user reaches for to ask "why didn't the hook inject X" answer
            // "nothing found" when the real answer was a refused pack. An
            // end-to-end test caught it: corrupting the pack's analyzer
            // produced an empty result and no message, which is exactly the
            // shape of an honest no-match.
            //
            // `search_with_report` rather than `search`, for the same reason:
            // a store-only index (no pack) at a profile that asks for BM25
            // hits `Store::fts_search`, which always errors now that lbug's
            // FTS index is gone — the pipeline degrades to vector-only rather
            // than failing, and this is the same command whose whole purpose
            // is answering "why didn't the hook inject X". Silently returning
            // vector-only results here would hide the answer from the one
            // place a user would look for it.
            let hits = match build_retriever(&cfg, &profile)
                .and_then(|r| r.search_with_report(&q, &profile))
            {
                Ok((hits, report)) => {
                    // The DEADLINE degradation, and this command is the one
                    // that most needs it. `hook::run_prompt` has printed this
                    // since it was written; `rhebok search` did not, and the
                    // asymmetry was backwards: the hook's user sees a prompt
                    // that quietly lacked a document, while THIS command's
                    // whole purpose (see the comment above) is answering "why
                    // didn't the hook inject X". A silent stage skip here made
                    // the diagnostic tool hide the diagnosis.
                    //
                    // Measured on the live index, tier 1, one query over 14
                    // runs: the single run that took 384ms — past the 220ms
                    // budget — returned a result set missing a document the
                    // other 13 all found, with nothing printed. It reads as
                    // run-to-run nondeterminism in retrieval, which is the
                    // wrong place to go looking.
                    if report.degraded {
                        eprintln!(
                            "rhebok: retrieval degraded — budget {}ms exceeded, ran only [{}]",
                            profile.budget_ms,
                            report.stages_run.join("+")
                        );
                    }
                    if let Some(reason) = &report.bm25_unavailable {
                        eprintln!(
                            "rhebok: retrieval degraded — bm25 unavailable, ran vector-only ({reason})"
                        );
                    }
                    // Mirror image of the check above — see
                    // `StageReport::vectors_unavailable`.
                    if let Some(reason) = &report.vectors_unavailable {
                        eprintln!(
                            "rhebok: retrieval degraded — vectors unavailable, ran bm25-only ({reason})"
                        );
                    }
                    if let Some(reason) = &report.memory_unavailable {
                        eprintln!(
                            "rhebok: memory unavailable — {reason}; run `rhebok memory rebuild`"
                        );
                    }
                    hits
                }
                Err(e) => {
                    eprintln!("rhebok: search unavailable — {e:#}");
                    Vec::new()
                }
            };

            if json {
                let results: Vec<serde_json::Value> = hits
                    .iter()
                    .map(|h| {
                        serde_json::json!({
                            "uri": escape_reserved_tags(&h.uri),
                            "title": escape_reserved_tags(&h.title),
                            "heading": escape_reserved_tags(&h.heading_path),
                            "page": h.page_no,
                            // Both, explicitly named. `relevance` is the [0,1]
                            // cosine a human or a model can interpret; `score`
                            // only orders, and after reranking it is a binary
                            // verdict. Emitting `score` alone under that name
                            // made every result at tier 3 read as 1.000.
                            "relevance": h.relevance, "score": h.score,
                            "text": escape_reserved_tags(&h.text),
                        })
                    })
                    .collect();
                println!("{}", serde_json::json!({ "results": results }));
            } else if hits.is_empty() {
                println!("no results");
            } else {
                print!("{CONTEXT_PREAMBLE}");
                let styled = {
                    use std::io::IsTerminal;
                    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
                };
                for h in &hits {
                    // `relevance`, not `score`: CLI search runs at the MCP tier,
                    // which reranks, and a reranked `score` is 1.0 or 0.0 — so
                    // every hit printed as (1.000), including irrelevant ones.
                    println!("{}", escape_reserved_tags(&render_hit(h, styled)));
                }
            }
        }

        Cmd::Doctor => {
            if !rhebok::doctor::run(&cfg)? {
                std::process::exit(1);
            }
        }
        Cmd::Context {
            json,
            prehook,
            session,
            cwd,
        } => {
            let cwd = cwd.or_else(|| std::env::current_dir().ok());
            let mut prompt = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut prompt)?;
            let context = if prehook {
                rhebok::context::build_for_prehook(
                    &prompt,
                    cwd.as_deref(),
                    session.as_deref(),
                    |profile| {
                        rhebok::retrieve_for_profile(&cfg, rhebok::config::Surface::Hook, profile)
                    },
                )
            } else {
                rhebok::context::build(&cfg, &prompt, cwd.as_deref())
            };
            if json {
                println!("{}", context.to_json());
            } else {
                for note in &context.notes {
                    eprintln!("{note}");
                }
                if let Some(block) = &context.block {
                    println!("{block}");
                }
            }
        }
        Cmd::Status => {
            let cfg_path = Config::config_path();
            let db = Config::db_path();
            let opened = Store::open_existing(&db, cfg.embed.dimensions);
            // A missing index (fresh machine, `rhebok index` never run) is a
            // normal empty state — `open_existing`'s own "no index at ..."
            // error names it. Any other failure to open — most often
            // `LOAD EXTENSION vector` refusing to load — is a broken build,
            // and must say so on both stderr and in the status line itself.
            // Rendering it as "documents: 0" would read as an empty knowledge
            // base instead of a link/toolchain problem the user needs to fix.
            let store_broken = match &opened {
                Ok(_) => None,
                Err(e) if e.downcast_ref::<rhebok::store::NoIndex>().is_some() => None,
                Err(e) => {
                    eprintln!("rhebok: status — the store could not be opened: {e:#}");
                    Some(
                        e.downcast_ref::<rhebok::store::VectorExtensionMissing>()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "could not open (see stderr)".to_string()),
                    )
                }
            };
            let snap = opened
                .map(|s| s.status_snapshot())
                .unwrap_or(StatusSnapshot {
                    documents: 0,
                    chunks: 0,
                    model: None,
                    skipped: Vec::new(),
                    vectors_pending: 0,
                });
            let (skipped, skipped_ignored) = rhebok::skip::split_ignored(snap.skipped, &cfg.ignore);
            let paths = rhebok::setup::Paths::from_env();
            let installed = env!("CARGO_PKG_VERSION");
            println!(
                "version:    {}",
                rhebok::update::version_line(
                    installed,
                    rhebok::update::UpdateCheck::read(&paths.update_json).as_ref(),
                    rhebok::update::Status::running(&paths.update_status).as_ref(),
                    rhebok::update::now(),
                )
            );
            let checks = rhebok::setup::install::install_checks(
                &paths,
                &rhebok::setup::claude::ClaudeCli::from_env(),
                installed,
                &std::env::current_exe().unwrap_or_default(),
            );
            let failed: Vec<_> = checks.iter().filter(|c| !c.ok).collect();
            if failed.is_empty() {
                println!("install:    {}  ok", paths.root.display());
            } else {
                println!(
                    "install:    {}  {} problem(s) — run rhebok install",
                    paths.root.display(),
                    failed.len()
                );
                for c in failed {
                    println!("  - {}: {}", c.name, c.detail);
                }
            }
            println!("config:     {}", cfg_path.display());
            let config_errors = rhebok::config::edit::read_document(&cfg_path)
                .ok()
                .flatten()
                .map(|text| rhebok::config::check::check(&text))
                .unwrap_or_default();
            if !config_errors.is_empty() {
                println!(
                    "            {} problem(s) — run rhebok config check; the hook ignores what it cannot read",
                    config_errors.len()
                );
                for e in &config_errors {
                    println!("  - {e}");
                }
            }
            println!("database:   {}", db.display());
            if let Some(why) = &store_broken {
                println!("store:      UNAVAILABLE — {why}; counts below are not meaningful");
            }
            println!("documents:  {}", snap.documents);
            match rhebok::memory::counts_at(&rhebok::memory::default_root()) {
                Ok(counts) => {
                    let mut kinds: Vec<String> = counts
                        .iter()
                        .map(|(k, n)| {
                            format!("{n} {}{}", k.as_str(), if *n == 1 { "" } else { "s" })
                        })
                        .collect();
                    kinds.sort();
                    if kinds.is_empty() {
                        println!("memory:     none");
                    } else {
                        println!("memory:     {}", kinds.join(", "));
                    }
                }
                Err(e) => {
                    eprintln!("rhebok: status — the memory store could not be read: {e:#}");
                    println!(
                        "memory:     UNAVAILABLE — could not read the memory store (see stderr)"
                    );
                }
            }
            // A half-embedded index that presents as complete is a worse
            // outcome than the slow synchronous index this task replaces, so
            // the backlog is always named explicitly rather than folded
            // silently into the chunk count.
            if snap.vectors_pending > 0 {
                println!(
                    "chunks:     {} ({} vectors pending — run `rhebok index --backfill`)",
                    snap.chunks, snap.vectors_pending
                );
            } else {
                println!("chunks:     {}", snap.chunks);
            }
            let db = Config::db_path();
            match rhebok::usage::load(&db) {
                Ok(map) => println!(
                    "usage:      {} documents retrieved, {} records waiting for the next index",
                    map.len(),
                    rhebok::usage::pending(&db)
                ),
                Err(e) => println!("usage:      UNAVAILABLE — {e:#}"),
            }
            println!(
                "model:      {} (configured: {})",
                snap.model.as_deref().unwrap_or("-"),
                cfg.embed.model
            );
            match (&cfg.embed.remote, &cfg.embed.remote_error) {
                (_, Some(e)) => println!("embed:      MISCONFIGURED — {e}"),
                (Some(r), None) => {
                    println!("embed:      {} (model {}; ollama bypassed)", r.url, r.model)
                }
                (None, None) => println!("embed:      {} (ollama)", cfg.embed.ollama_url),
            }
            let hq = cfg.quality_for(Surface::Hook);
            let mq = cfg.quality_for(Surface::Mcp);
            println!("hook:       tier {hq} ({})", Profile::tier(hq).name);
            println!("prehook:    {}", cfg.prehook.injector.describe());
            if let Ok(here) = std::env::current_dir() {
                for line in
                    rhebok::project::describe(&rhebok::project::resolve(&cfg, &here), &here).lines()
                {
                    println!("project:    {line}");
                }
            }
            println!("mcp:        tier {mq} ({})", Profile::tier(mq).name);
            println!("sources:    {}", cfg.sources.len());
            #[cfg(feature = "backup")]
            if rhebok::backup::is_configured(&cfg) {
                println!(
                    "backup:     {} ({})",
                    rhebok::backup::describe_age(rhebok::backup::read_stamp(&db)),
                    cfg.backup.targets.join(", ")
                );
            }

            for line in rhebok::setup::ollama::status_lines(
                &cfg.embed.ollama_url,
                &rhebok::setup::ollama::ollama_models(&cfg),
            ) {
                println!("ollama:     {line}");
            }

            // Scanned PDFs are read only if two external libraries are present,
            // and neither is discoverable by default on macOS. Reporting them
            // here is the difference between "OCR is on" and OCR actually
            // having a chance to run. Location only — a library that is found
            // can still fail to load, so this never claims OCR works.
            match cfg.pdf.ocr {
                rhebok::config::OcrMode::Off => println!("ocr:        off"),
                mode => {
                    let libs = rhebok::loaders::pdf::ocr_library_paths(&cfg.pdf);
                    let missing: Vec<&str> = libs
                        .iter()
                        .filter(|(_, path)| path.is_none())
                        .map(|(name, _)| *name)
                        .collect();
                    let mode = if matches!(mode, rhebok::config::OcrMode::Force) {
                        "force"
                    } else {
                        "auto"
                    };
                    // Reported beside the library paths for the same reason
                    // they are: an offline install that silently tried the
                    // network, or a locked-down one that silently did not,
                    // both look like "OCR did nothing".
                    //
                    // UNCONDITIONAL, symmetric with the `ocr:` line below.
                    // Printing only the offline case meant a user who had
                    // explicitly set `if-missing` saw exactly what a user who
                    // had set nothing saw, so the setting could not be
                    // confirmed to have taken effect from the tool's own
                    // output — which is the shape of thing this project's
                    // failure culture exists to refuse.
                    println!(
                        "ocr models: {}",
                        match cfg.pdf.model_downloads {
                            rhebok::config::ModelDownloads::Offline => "offline (never downloaded)",
                            rhebok::config::ModelDownloads::IfMissing =>
                                "if-missing (downloaded on first use)",
                        }
                    );
                    println!(
                        "ocr:        {}",
                        rhebok::loaders::pdf::describe_ocr(mode, &cfg.pdf, &missing)
                    );
                }
            }

            // A run in progress. The SessionStart indexer is detached and its
            // stderr goes to a log file, so without this there is no way to ask
            // how far a background index has got — or whether one is running.
            if let Some(p) = read_progress(&db.with_extension("progress")) {
                println!("indexing:   {p}");
            } else if rhebok::index::IndexLock::is_held(&db) {
                println!("indexing:   starting…");
            }
            // The last run's skips. Without this the only report was an
            // `eprintln!` from a process whose stderr goes to a log nobody
            // opens, so a scanned PDF vanished from search with no explanation.
            if skipped.is_empty() && skipped_ignored == 0 {
                println!("skipped:    none");
            } else if skipped.is_empty() {
                println!("skipped:    none shown, {skipped_ignored} matched by `ignore`");
            } else {
                println!("skipped:    {} (not indexed)", skipped.len());
                for line in skipped.iter().take(10) {
                    println!("  - {line}");
                }
                if skipped.len() > 10 {
                    println!("  … and {} more", skipped.len() - 10);
                }
            }

            match std::env::current_exe().and_then(|p| rhebok::sha256::hex_of_file(&p)) {
                Ok(h) => println!("build:      {h}"),
                Err(e) => {
                    println!("build:      unknown (could not read this binary's own bytes: {e})")
                }
            }
            match std::fs::metadata(&paths.bin) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => println!(
                    "hook binary:{} — not installed yet (run `rhebok install`)",
                    paths.bin.display()
                ),
                Err(e) => println!("hook binary:{} — could not read ({e})", paths.bin.display()),
                Ok(_) => {
                    let theirs = match rhebok::sha256::hex_of_file(&paths.bin) {
                        Ok(h) => h,
                        Err(e) => {
                            println!("hook binary:{} — could not read ({e})", paths.bin.display());
                            return Ok(());
                        }
                    };
                    match std::env::current_exe() {
                        Err(e) => println!(
                            "hook binary:{} — could not determine this binary's own path to compare ({e})",
                            paths.bin.display()
                        ),
                        Ok(mine_path) => match rhebok::sha256::hex_of_file(&mine_path) {
                            Err(e) => println!(
                                "hook binary:{} — could not read this binary's own bytes to compare ({e})",
                                paths.bin.display()
                            ),
                            Ok(mine) => {
                                let verdict = if theirs == mine {
                                    "same as this binary"
                                } else {
                                    "MISMATCH — the hook runs a different build"
                                };
                                println!("hook binary:{} — {verdict}", paths.bin.display());
                                if theirs != mine {
                                    println!(
                                        "            fix: rm -f {} && cp {} {}",
                                        paths.bin.display(),
                                        mine_path.display(),
                                        paths.bin.display()
                                    );
                                }
                            }
                        },
                    }
                }
            }
        }

        Cmd::Add { target } => {
            let doc = if target.starts_with("http") {
                rhebok::loaders::web::WebLoader::fetch(&target)?
            } else {
                let p = std::path::Path::new(&target);
                if p.extension().and_then(|e| e.to_str()) == Some("pdf") {
                    rhebok::loaders::pdf::PdfLoader::load_file_with(p, &cfg.pdf)?
                } else {
                    rhebok::loaders::markdown::MarkdownLoader::load_file(p)?
                }
            };
            // `add` writes straight into the live database, so it must hold
            // the SAME lock `reindex_swap` takes. Without it, an add landing
            // after the swap copied live -> shadow was written to a database
            // about to be renamed away: the document vanished with no error.
            // `SessionStart` spawns an indexer every session, so that race is
            // routine, not theoretical.
            let _lock = rhebok::index::IndexLock::acquire(&Config::db_path()).ok_or_else(|| {
                anyhow::anyhow!("`rhebok index` is running; try again once it finishes")
            })?;
            let idx = build_indexer(&cfg)?;
            let stats = idx.index_documents(std::slice::from_ref(&doc))?;

            // Resolve against the WHOLE corpus, not just this document.
            // `upsert_document` DETACH DELETEs the node, which drops INBOUND edges
            // too — so re-adding a note that other notes link TO severs their links.
            // Measured: A->B was 1, dropped to 0 after `rhebok add` of B, and only
            // returned after the next full index. Passing every document lets those
            // inbound links be rebuilt immediately.
            let all = discover(&cfg).unwrap_or_default();
            let mut corpus = all;
            if !corpus.iter().any(|d| d.id == doc.id) {
                corpus.push(doc.clone());
            }
            idx.resolve_links(&corpus)?;
            print_ambiguous_titles(&idx);

            println!("added `{}` ({} chunks)", doc.title, stats.chunks);
            // `add` writes straight into the live database and never touches
            // the pack, which is only ever built in a shadow directory and
            // published by rename (see `reindex_swap_with`) — writing it into
            // the live directory here would break that atomicity guarantee.
            // Once retrieval reads the pack instead of the database, this
            // document is invisible to search until the next `rhebok index`.
            // Silence here would be exactly the silent-degradation failure
            // mode this project exists to avoid, so say so on stderr.
            eprintln!(
                "note: `{}` will not be searchable until the next `rhebok index`",
                doc.title
            );
        }

        Cmd::Bench {
            synthetic: Some(chunks),
            seed,
            out,
            reuse,
            json,
            ..
        } => {
            let report = match reuse {
                Some(dir) => rhebok::bench::synthetic::run_reuse(&cfg, chunks, seed, &dir)?,
                None => rhebok::bench::synthetic::run(&cfg, chunks, seed, out.as_deref())?,
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", rhebok::bench::synthetic::render(&report));
            }
        }
        Cmd::Bench {
            no_graph,
            keep_footprint,
            project,
            ..
        } => rhebok::bench::run(&cfg, no_graph, keep_footprint, project.as_deref())?,
        // `check` returns `Err` when it finds an error, which is what makes the
        // process exit non-zero — the report itself is already printed by then,
        // so the `Error:` line anyhow adds is a summary and not the finding.
        Cmd::Config { what } => {
            if !run_config(what)? {
                std::process::exit(1);
            }
        }
        Cmd::Golden { what } => match what {
            GoldenCmd::Init { force } => rhebok::golden::init(&cfg, force)?,
            GoldenCmd::Check => rhebok::golden::check(&cfg)?,
        },
        Cmd::Project { what } => match what {
            ProjectCmd::List => {
                if cfg.projects.is_empty() {
                    println!("no projects in {}", Config::config_path().display());
                }
                for (name, project) in &cfg.projects {
                    let roots: Vec<String> = project
                        .roots
                        .iter()
                        .map(|r| r.display().to_string())
                        .collect();
                    println!("{name}: {}", roots.join(", "));
                }
            }
            ProjectCmd::Show { cwd } => {
                let cwd = cwd
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_default();
                let resolution = rhebok::project::resolve(&cfg, &cwd);
                print!("{}", rhebok::project::describe(&resolution, &cwd));
                if let Some(project) = &resolution.project {
                    for line in rhebok::project::import_lines(&cfg, project) {
                        println!("{line}");
                    }
                }
            }
            ProjectCmd::Trust { path } => {
                let file = rhebok::project::trust_in(&rhebok::project::trust_store_path(), &path)?;
                println!(
                    "trusted {}; editing it untrusts it until you run this again",
                    file.display()
                );
                match rhebok::index::rescope(&cfg) {
                    Ok(_) => println!("project scopes rebuilt"),
                    Err(e) => eprintln!(
                        "rhebok: scopes not rebuilt yet ({e:#}); run `rhebok project rescope`"
                    ),
                }
            }
            ProjectCmd::Explain { target } => {
                print!("{}", rhebok::project::explain(&cfg, &target)?)
            }
            ProjectCmd::Export {
                name,
                out,
                include_transcripts,
                dry_run,
            } => {
                if dry_run {
                    let (_, plan) = rhebok::project::plan_export(&cfg, &name, include_transcripts)?;
                    println!(
                        "would export {} chunk(s) from {} document(s):",
                        plan.rows.len(),
                        plan.documents.len()
                    );
                    for uri in &plan.documents {
                        println!("  {uri}");
                    }
                    if plan.transcripts_left_out > 0 {
                        println!(
                            "left out {} session transcript(s); --include-transcripts adds them",
                            plan.transcripts_left_out
                        );
                    }
                } else {
                    let plan = rhebok::project::export(&cfg, &name, &out, include_transcripts)?;
                    println!(
                        "exported {} chunk(s) from {} document(s) to {}",
                        plan.rows.len(),
                        plan.documents.len(),
                        out.display()
                    );
                    if plan.transcripts_left_out > 0 {
                        println!(
                            "left out {} session transcript(s); --include-transcripts adds them",
                            plan.transcripts_left_out
                        );
                    }
                }
            }
            ProjectCmd::Mount { dir, name } => {
                let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
                let slice = rhebok::project::read_slice(&dir)?;
                rhebok::project::open_mount(&cfg, &dir)?;
                let name = name.unwrap_or(slice.name);
                let path = Config::config_path();
                let patch = rhebok::config::edit::Patch {
                    set: vec![(
                        format!("mounts.{name}.path"),
                        toml_edit::value(dir.display().to_string()),
                    )],
                    unset: Vec::new(),
                };
                rhebok::config::edit::update(&path, None, &patch)?;
                println!(
                    "mounted {} as `{name}` ({} documents); a project imports it with `imports = [\"{name}\"]`",
                    dir.display(),
                    slice.documents
                );
            }
            ProjectCmd::Unmount { name } => {
                let path = Config::config_path();
                let patch = rhebok::config::edit::Patch {
                    set: Vec::new(),
                    unset: vec![format!("mounts.{name}")],
                };
                rhebok::config::edit::update(&path, None, &patch)?;
                println!("unmounted `{name}`");
            }
            ProjectCmd::Instructions { name } => {
                let text = rhebok::project::chat_instructions(&cfg, &name)?;
                let env = rhebok::setup::agents::AgentEnv::from_env();
                if rhebok::setup::agents::claude_desktop::ClaudeDesktop::rhebok_status(&env)
                    != rhebok::setup::agents::Status::Connected
                {
                    eprintln!(
                        "rhebok: Claude Desktop does not have rhebok connected yet, so these \
                         instructions would call a tool it cannot see; run \
                         `rhebok connect claude-desktop` and restart Claude Desktop"
                    );
                }
                eprintln!(
                    "Paste the text below into the Chat Project's instructions in Claude Desktop:\n"
                );
                print!("{text}");
            }
            ProjectCmd::Rescope => {
                let started = std::time::Instant::now();
                let n = rhebok::index::rescope(&cfg)?;
                println!(
                    "rebuilt {n} project scope(s) in {} ms",
                    started.elapsed().as_millis()
                );
            }
        },
        Cmd::Memory { what } => match what {
            MemoryCmd::Add {
                kind,
                project,
                global,
                title,
                text,
            } => {
                let kind = rhebok::memory::MemoryKind::parse(&kind)
                    .ok_or_else(|| anyhow::anyhow!("--kind must be lesson, fact or episode"))?;
                let project = match (global, project) {
                    (true, _) => None,
                    (false, Some(p)) => Some(p),
                    (false, None) => Some(std::env::current_dir()?),
                };
                let outcome = rhebok::memory::remember(
                    &cfg,
                    rhebok::memory::Remember {
                        kind,
                        text: text.join(" "),
                        title,
                        project,
                        confidence: 100,
                        origin: rhebok::memory::Origin::User,
                        session: None,
                        source_hash: None,
                        source_stamp: None,
                        created: None,
                    },
                )?;
                println!("{}", outcome.describe(kind));
                if matches!(outcome, rhebok::memory::Outcome::Rejected(_)) {
                    std::process::exit(1);
                }
            }
            MemoryCmd::List {
                kind,
                project,
                json,
            } => {
                let kind = match kind {
                    Some(k) => Some(rhebok::memory::MemoryKind::parse(&k).ok_or_else(|| {
                        anyhow::anyhow!("--kind must be lesson, fact or episode")
                    })?),
                    None => None,
                };
                let all = rhebok::memory::list(&cfg, &rhebok::memory::Filter { kind, project })?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&all)?);
                } else if all.is_empty() {
                    println!("no memories yet");
                } else {
                    for m in &all {
                        let scope = m.facts.project.as_deref().unwrap_or("global");
                        println!(
                            "{}  {:<7} {}  {}  {}",
                            m.id,
                            m.facts.kind.as_str(),
                            rhebok::memory::ymd(m.facts.created),
                            scope,
                            m.text.lines().next().unwrap_or("")
                        );
                    }
                }
            }
            MemoryCmd::Forget { id } => {
                let gone = rhebok::memory::forget(&cfg, &id)?;
                println!(
                    "forgot {} {}: {}",
                    gone.facts.kind.as_str(),
                    gone.id,
                    gone.title
                );
            }
            MemoryCmd::Promote { id } => {
                let out = rhebok::memory::promote(&cfg, &id)?;
                match out {
                    rhebok::memory::Outcome::Rejected(reason) => println!("{reason}"),
                    _ => println!("promoted {id}: it now applies in every project"),
                }
            }
            MemoryCmd::Distill { all, session } => {
                if let Some(path) = session {
                    let out = rhebok::memory::distill::distill_session(&cfg, &path)?;
                    println!("{}", out.describe(rhebok::memory::MemoryKind::Episode));
                } else {
                    let limit = if all { usize::MAX } else { 1 };
                    let r = rhebok::memory::distill::distill_pending(&cfg, limit, false)?;
                    println!(
                        "distilled {} of {} pending session(s){}",
                        r.distilled,
                        r.candidates,
                        r.latched
                            .as_deref()
                            .map(|l| format!(" — stopped: {l}"))
                            .unwrap_or_default()
                    );
                }
            }
            MemoryCmd::Export { out } => {
                let rows = rhebok::memory::export(&cfg)?;
                let mut text = String::new();
                for r in &rows {
                    text.push_str(&serde_json::to_string(r)?);
                    text.push('\n');
                }
                match out {
                    Some(p) => {
                        std::fs::write(&p, text)?;
                        println!("wrote {} memories to {}", rows.len(), p.display());
                    }
                    None => print!("{text}"),
                }
            }
            MemoryCmd::Import { file } => {
                let text = std::fs::read_to_string(&file)?;
                let rows: Vec<rhebok::memory::ExportRow> = text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(serde_json::from_str)
                    .collect::<Result<_, _>>()?;
                let (saved, skipped) = rhebok::memory::import(&cfg, &rows)?;
                println!("imported {saved} memories, skipped {skipped} duplicates");
            }
            MemoryCmd::Rebuild => {
                let n = rhebok::memory::rebuild(&cfg)?;
                println!("rebuilt {n} memories with {}", cfg.embed.model);
            }
        },
        Cmd::AuditInjections { root } => {
            let root =
                root.unwrap_or_else(rhebok::loaders::transcript::TranscriptLoader::default_root);
            let a = rhebok::audit::audit(&root)?;
            if a.injections == 0 {
                println!("no injections found under {}", root.display());
                println!("(the hook writes its block into the session transcript; if you have");
                println!(" never run a prompt with the hook installed, this is expected)");
                return Ok(());
            }
            println!("injections:  {}", a.injections);
            println!("entries:     {}", a.entries);
            for (scheme, n) in &a.by_scheme {
                let label = if scheme == "file" {
                    "vault note"
                } else if scheme == "claude-session" {
                    "transcript"
                } else if scheme == "codex-session" {
                    "codex session"
                } else {
                    scheme
                };
                println!(
                    "  {label:<12} {n:>6}  ({:.1}%)",
                    100.0 * *n as f64 / a.entries.max(1) as f64
                );
            }
            println!(
                "injections with no vault note: {} ({:.0}%)",
                a.injections_with_no_note,
                100.0 * a.injections_with_no_note as f64 / a.injections as f64
            );
            let mut b = a.body_bytes.clone();
            b.sort_unstable();
            if !b.is_empty() {
                let q = |p: f64| b[((b.len() - 1) as f64 * p) as usize];
                println!(
                    "body bytes:  p25 {}  p50 {}  p90 {}",
                    q(0.25),
                    q(0.50),
                    q(0.90)
                );
            }
        }
        Cmd::Mcp => rhebok::mcp::serve(cfg)?,
        Cmd::Hook { which, agent } => rhebok::hook::run_for(
            &which,
            rhebok::hook::PromptAgent::parse(&agent)
                .unwrap_or(rhebok::hook::PromptAgent::ClaudeCode),
            &cfg,
        ),
        Cmd::App { args } => {
            use std::os::unix::process::CommandExt;
            let paths = rhebok::setup::Paths::from_env();
            let beside_cli = std::env::current_exe()
                .ok()
                .map(|exe| exe.with_file_name("rhebok-app"));
            let bundle = paths.app_bundle();
            let ours = rhebok::setup::app::existing(&bundle) == rhebok::setup::app::Existing::Ours;
            let app = [
                ours.then(|| bundle.join(rhebok::setup::app::APP_EXECUTABLE)),
                Some(paths.root.join("bin/rhebok-app")),
                beside_cli,
            ]
            .into_iter()
            .flatten()
            .find(|candidate| candidate.exists());
            let Some(app) = app else {
                eprintln!(
                    "{}",
                    rhebok::setup::app::not_installed_message(
                        cfg!(target_os = "macos"),
                        rhebok::setup::app::RELEASE_BUILD
                    )
                );
                std::process::exit(1);
            };
            let err = std::process::Command::new(&app).args(&args).exec();
            anyhow::bail!("could not start {}: {err}", app.display());
        }
        Cmd::Dashboard { port, no_open } => {
            rhebok::dashboard::serve(port, !no_open)?;
        }
        Cmd::Restart => {
            let paths = rhebok::setup::Paths::from_env();
            rhebok::restart::restart(&paths, &mut std::io::stdout())?;
        }
        Cmd::Install {
            yes,
            quiet,
            embed_model,
        } => {
            let asked = embed_model
                .as_deref()
                .map(rhebok::embed::source::parse)
                .transpose()?;
            let paths = rhebok::setup::Paths::from_env();
            let agent_env = rhebok::setup::agents::AgentEnv::from_env();
            let migration = rhebok::setup::migrate::migrate(
                &paths,
                &Config::config_path(),
                &rhebok::setup::claude::ClaudeCli::from_env(),
                &agent_env,
                &rhebok::setup::migrate::Crontab::system(),
            )?;
            let mut migration = migration;
            migration
                .warnings
                .extend(rhebok::setup::migrate::legacy_env_warnings(
                    std::env::vars().map(|(k, _)| k),
                ));
            let cfg = if migration.migrated() {
                Config::load()
            } else {
                cfg
            };
            let first_install = !Config::config_path().is_file();
            let interactive = !yes && !quiet && std::io::IsTerminal::is_terminal(&std::io::stdin());
            let remote = cfg.embed.remote.is_some() || cfg.embed.remote_error.is_some();
            let source = match asked {
                Some(source) => Some(source),
                None if !remote && !yes && !quiet && first_install => Some(choose_embed_on_stdin()),
                None => None,
            };
            use rhebok::embed::source::ModelSource;
            let mut embed = None;
            let mut custom_ollama = None;
            let mut endpoint = None;
            match source {
                Some(ModelSource::Endpoint(url)) => {
                    endpoint = Some(connect_endpoint(&url, interactive)?);
                }
                Some(other) if remote => {
                    migration.warnings.push(format!(
                        "a remote embedding endpoint is configured, so {} was ignored; \
                         --embed-model with an Ollama model only chooses the model Ollama serves",
                        other.describe()
                    ));
                }
                Some(ModelSource::Preset(choice)) => embed = Some(choice),
                Some(ModelSource::Ollama(model)) => custom_ollama = Some(model),
                None => {}
            }
            let cfg = if endpoint.is_some() {
                Config::load()
            } else {
                cfg
            };
            let mut model_cfg = cfg.clone();
            if let Some(choice) = embed {
                model_cfg.embed.model = choice.model.to_string();
            }
            if let Some(model) = &custom_ollama {
                model_cfg.embed.model = model.clone();
            }
            let opts = rhebok::setup::install::InstallOpts {
                exe: std::env::current_exe()?,
                version: env!("CARGO_PKG_VERSION").to_string(),
                config_path: Config::config_path(),
                claude: rhebok::setup::claude::ClaudeCli::from_env(),
                yes,
                quiet,
                ollama_url: cfg.embed.ollama_url.clone(),
                models: rhebok::setup::ollama::ollama_models(&model_cfg),
                embed,
                confirm: confirm_on_stdin,
                pull: rhebok::setup::ollama::pull,
                app: cfg!(target_os = "macos").then(|| rhebok::setup::app::AppOpts {
                    target: env!("RHEBOK_TARGET").to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    exe: std::env::current_exe().unwrap_or_default(),
                    download: rhebok::setup::app::RELEASE_BUILD.then(|| {
                        rhebok::setup::app::Download {
                            api: rhebok::update::release::api_base(),
                            token: rhebok::update::release::token(),
                        }
                    }),
                }),
            };
            let mut report = rhebok::setup::install::install(&paths, &opts)?;
            if let Some(model) = &custom_ollama {
                match rhebok::embed::probe::ollama_dimensions(&cfg.embed.ollama_url, model) {
                    Ok(dimensions) => rhebok::setup::model::apply_custom(
                        &opts.config_path,
                        model,
                        dimensions,
                        first_install,
                        rhebok::setup::model::measure_gates,
                        &mut report,
                    ),
                    Err(e) => report.warnings.push(format!(
                        "{e:#}. rhebok still embeds with {}; once `ollama pull {model}` works, \
                         run `rhebok install --embed-model {model}`",
                        Config::load_from(&opts.config_path).embed.model
                    )),
                }
            }
            if let Some(ep) = &endpoint {
                report.lines.push(format!(
                    "endpoint: {} serves {}, {}",
                    ep.url,
                    ep.model,
                    if ep.token.is_some() {
                        "with a token"
                    } else {
                        "with no token"
                    }
                ));
                rhebok::setup::model::apply_custom(
                    &opts.config_path,
                    &ep.model,
                    ep.dimensions,
                    first_install,
                    rhebok::setup::model::measure_gates,
                    &mut report,
                );
            }
            let offered = rhebok::setup::agents::offer_after_install(
                &agent_env,
                yes,
                quiet,
                confirm_on_stdin,
            );
            if !quiet {
                for l in migration
                    .lines
                    .iter()
                    .chain(&report.lines)
                    .chain(&offered.lines)
                {
                    println!("{l}");
                }
            }
            let warnings: Vec<&String> = migration
                .warnings
                .iter()
                .chain(&report.warnings)
                .chain(&offered.warnings)
                .collect();
            for w in &warnings {
                eprintln!("! {w}");
            }
            if !quiet {
                println!(
                    "{}",
                    rhebok::setup::install::next_steps(&opts.config_path, warnings.len())
                );
            }
        }
        Cmd::Agents { json } => run_agents(json)?,
        Cmd::Connect {
            agents,
            instructions,
            all_detected,
            print,
        } => run_connect(agents, instructions, all_detected, print)?,
        Cmd::Disconnect { agents } => run_disconnect(agents)?,
        Cmd::Uninstall { purge, yes } => {
            rhebok::setup::agents::refuse_uninstall_while_blocked(
                &rhebok::setup::agents::AgentEnv::from_env(),
            )?;
            let paths = rhebok::setup::Paths::from_env();
            let opts = rhebok::setup::install::UninstallOpts {
                purge,
                yes,
                claude: rhebok::setup::claude::ClaudeCli::from_env(),
                config_path: Config::config_path(),
                confirm: confirm_on_stdin,
            };
            let report = rhebok::setup::install::uninstall(&paths, &opts)?;
            let agents = rhebok::setup::agents::disconnect_all_but_claude_code(
                &rhebok::setup::agents::AgentEnv::from_env(),
            );
            for l in report.lines.iter().chain(&agents.lines) {
                println!("{l}");
            }
            for w in report.warnings.iter().chain(&agents.warnings) {
                eprintln!("! {w}");
            }
            if !report.kept.is_empty() {
                println!("kept (remove with `rhebok uninstall --purge`):");
                for k in &report.kept {
                    println!("  {}", k.display());
                }
            }
        }
        Cmd::Update {
            check,
            yes: _,
            quiet,
        } => {
            let opts = rhebok::update::UpdateOpts {
                paths: rhebok::setup::Paths::from_env(),
                installed: env!("CARGO_PKG_VERSION").to_string(),
                api: rhebok::update::release::api_base(),
                token: rhebok::update::release::token(),
                target: env!("RHEBOK_TARGET").to_string(),
                check_only: check,
            };
            match rhebok::update::run(&opts)? {
                rhebok::update::Outcome::CheckOnly(c) => {
                    if !quiet {
                        if c.no_release_published() {
                            println!(
                                "installed {}; {}",
                                c.installed,
                                rhebok::update::NO_RELEASE_PUBLISHED
                            );
                        } else {
                            match c.available() {
                                Some(v) => println!(
                                    "installed {}, latest {v} — run `rhebok update`",
                                    c.installed
                                ),
                                None => println!(
                                    "installed {}, latest {} — up to date",
                                    c.installed,
                                    c.latest.as_deref().unwrap_or("?")
                                ),
                            }
                        }
                    }
                }
                rhebok::update::Outcome::NoReleasePublished(v) => {
                    if !quiet {
                        println!("rhebok {v}: {}", rhebok::update::NO_RELEASE_PUBLISHED);
                    }
                }
                rhebok::update::Outcome::UpToDate(v) => {
                    if !quiet {
                        println!("rhebok {v} is up to date");
                    }
                }
                rhebok::update::Outcome::Updated { from, to } => {
                    println!(
                        "updated {from} -> {to}; restart Claude Code so the new version loads"
                    );
                }
            }
        }
        #[cfg(feature = "backup")]
        Cmd::Backup { action } => run_backup(&cfg, action)?,
        #[cfg(feature = "backup")]
        Cmd::Restore {
            generation,
            index,
            dry_run,
            force,
            memories,
            allow_unauthenticated_manifest,
            allow_unencrypted_manifest,
        } => run_restore(
            &cfg,
            rhebok::backup::RestoreOptions {
                generation,
                index,
                dry_run,
                force,
                memories,
                allow_unauthenticated_manifest,
                allow_unencrypted_manifest,
            },
        )?,
    }
    Ok(())
}

fn run_index(
    cfg: &Config,
    reindex: bool,
    compact: bool,
    no_embed: bool,
    backfill: bool,
) -> Result<()> {
    rhebok::index::lower_priority();
    if backfill {
        // Phase 2: drain whatever `--no-embed` (or a previous,
        // interrupted `--backfill`) left behind. Runs to completion —
        // an unbounded budget — because this is a direct, foreground
        // request; it still yields `IndexLock` and the store itself
        // between short batches the whole way, so it never blocks a
        // concurrent reader or writer for its own duration. See
        // `index::backfill_vectors`.
        let stats = rhebok::index::backfill_vectors(cfg, std::time::Duration::MAX)?;
        println!(
            "backfilled: {} embedded, {} still pending{}",
            stats.embedded,
            stats.remaining,
            if stats.republished {
                " (pack republished)"
            } else {
                ""
            }
        );
        return Ok(());
    }

    // Builds into a shadow directory and swaps it in with a rename, so
    // the hook (a separate process) never sees the database mid-write.
    // LadybugDB takes an exclusive OS file lock, so indexing in place
    // would leave the hook returning nothing, silently, for the whole
    // run. See `index::reindex_swap`.
    //
    // `--reindex` used to `remove_dir_all` the live database HERE,
    // before `reindex_swap` took the writer lock — deleting the
    // directory out from under a concurrent indexer mid-swap. That is
    // the same race the lock exists to close, moved one line earlier.
    // It is now expressed as "do not seed the shadow from the live
    // index", which needs no wipe at all and happens under the lock.
    let mode = if compact {
        RebuildMode::Compact
    } else if reindex {
        RebuildMode::FromScratch
    } else {
        RebuildMode::Incremental
    };
    // `graph.kz` is what actually holds the space lbug cannot
    // reclaim, so the before/after line compaction reports is
    // measured against the whole store directory on disk, not any
    // in-memory estimate.
    let before_bytes = compact.then(|| rhebok::index::dir_size(&Config::db_path()));
    let stats = reindex_swap_with(cfg, mode, !no_embed)?;
    println!(
        "indexed: {} added, {} updated, {} skipped, {} removed, {} chunks \
         ({} written, {} reused, {} pruned)",
        stats.added,
        stats.updated,
        stats.skipped,
        stats.deleted,
        stats.chunks,
        stats.chunks_written,
        stats.chunks_reused,
        stats.chunks_pruned
    );
    if no_embed {
        println!(
            "note: published with no embeddings; run `rhebok index --backfill` \
             to fill them in"
        );
    }
    if let Some(before) = before_bytes {
        let after = rhebok::index::dir_size(&Config::db_path());
        println!(
            "compacted: {:.1} MB -> {:.1} MB",
            before as f64 / 1_048_576.0,
            after as f64 / 1_048_576.0
        );
    }
    if rhebok::memory::distill::runs_automatically(cfg) && !no_embed {
        match rhebok::memory::distill::distill_pending(cfg, 1, true) {
            Ok(r) if r.skipped_idle => eprintln!(
                "rhebok: distillation skipped — a prompt was seen in the last {}s",
                cfg.memory.distill_idle_secs
            ),
            Ok(r) if r.distilled > 0 => eprintln!(
                "rhebok: distilled {} session(s), {} pending",
                r.distilled,
                r.candidates - r.distilled
            ),
            Ok(_) => {}
            Err(e) => eprintln!("rhebok: distillation failed — {e:#}"),
        }
    }
    Ok(())
}

fn run_config(what: ConfigCmd) -> Result<bool> {
    use rhebok::config::{check, edit};
    let path = Config::config_path();
    match what {
        ConfigCmd::Path => println!("{}", path.display()),
        ConfigCmd::Check => {
            let Some(text) = edit::read_document(&path)? else {
                println!(
                    "no config at {}; every setting is at its default",
                    path.display()
                );
                return Ok(true);
            };
            let errors = check::check(&text);
            if errors.is_empty() {
                println!("{}: ok", path.display());
                return Ok(true);
            }
            for e in &errors {
                println!("{}: {e}", path.display());
            }
            println!("{} problem(s)", errors.len());
            return Ok(false);
        }
        ConfigCmd::Get { key } => {
            let segments: Vec<&str> = key.split('.').collect();
            if rhebok::config::schema::config_schema()
                .at(&segments)
                .is_none()
            {
                eprintln!("rhebok: `{key}` is not a config key");
                return Ok(false);
            }
            let effective = rhebok::config::view::config_json(&Config::load_from(&path))?;
            let value = segments
                .iter()
                .try_fold(&effective, |v, k| v.get(*k))
                .unwrap_or(&serde_json::Value::Null);
            match value {
                serde_json::Value::Null => eprintln!("{key} is not set"),
                serde_json::Value::String(s) => println!("{s}"),
                serde_json::Value::Object(_) => print!(
                    "{}",
                    toml::to_string(&rhebok::config::view::without_nulls(value))?
                ),
                other => println!("{other}"),
            }
        }
        ConfigCmd::Set { key, value } => {
            let patch = edit::Patch {
                set: vec![(key.clone(), edit::value_from_cli(&value))],
                unset: Vec::new(),
            };
            return report_config_update(&path, &key, edit::update(&path, None, &patch)?);
        }
        ConfigCmd::Unset { key } => {
            let patch = edit::Patch {
                set: Vec::new(),
                unset: vec![key.clone()],
            };
            return report_config_update(&path, &key, edit::update(&path, None, &patch)?);
        }
    }
    Ok(true)
}

fn report_config_update(
    path: &std::path::Path,
    key: &str,
    outcome: rhebok::config::edit::Outcome,
) -> Result<bool> {
    use rhebok::config::edit::Outcome;
    match outcome {
        Outcome::Written { .. } => {
            println!("updated {key} in {}", path.display());
            Ok(true)
        }
        Outcome::Invalid(errors) => {
            for e in &errors {
                eprintln!("rhebok: {e}");
            }
            eprintln!("rhebok: {} was not changed", path.display());
            Ok(false)
        }
        Outcome::Conflict { .. } => {
            eprintln!(
                "rhebok: {} changed while it was being edited; try again",
                path.display()
            );
            Ok(false)
        }
    }
}

#[cfg(feature = "backup")]
fn run_backup(cfg: &Config, action: Option<BackupAction>) -> Result<()> {
    match action {
        None => backup_once(cfg),
        Some(BackupAction::Init { yes }) => backup_init(cfg, yes),
        Some(BackupAction::Check) => backup_check(cfg),
        Some(BackupAction::Auth { provider: _ }) => backup_auth_drive(cfg),
        Some(BackupAction::Status) => backup_status(cfg),
        Some(BackupAction::Schedule { at, uninstall }) => backup_schedule(&at, uninstall),
    }
}

#[cfg(feature = "backup")]
fn backup_once(cfg: &Config) -> Result<()> {
    let outcome = rhebok::backup::run_all(cfg);
    let line = match &outcome {
        rhebok::backup::RunOutcome::Done(targets) => {
            let summary = targets
                .iter()
                .map(|(name, s)| {
                    format!(
                        "{name}: {} uploaded, {} deduped, {} bytes, {}",
                        s.uploaded, s.deduped, s.bytes, s.generation
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!("ok  {summary}")
        }
        rhebok::backup::RunOutcome::Skipped(why) => format!("skip  {why}"),
        rhebok::backup::RunOutcome::Failed(why) => format!("fail  {why}"),
    };
    append_backup_log(&line);
    match outcome.exit_code() {
        0 => println!("{line}"),
        code => {
            eprintln!("{line}");
            std::process::exit(code);
        }
    }
    Ok(())
}

#[cfg(feature = "backup")]
fn backup_init(cfg: &Config, yes: bool) -> Result<()> {
    let path = cfg.backup_key_path();
    let key = rhebok::backup::crypto::Key::generate();
    key.save(&path)?;
    println!("backup key: {}", key.to_hex());
    println!();
    println!(
        "Written to {}. Please store this somewhere other than this machine: \
         a password manager, or paper.",
        path.display()
    );
    println!(
        "Without it, every backup made with this key is unrecoverable. `rhebok` cannot help you get it back."
    );
    if !yes {
        print!("Type `yes` once you have stored it: ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if answer.trim() != "yes" {
            eprintln!("not confirmed; the key is still at {}", path.display());
            std::process::exit(1);
        }
    }
    Ok(())
}

#[cfg(feature = "backup")]
fn backup_check(cfg: &Config) -> Result<()> {
    rhebok::backup::scrub_credential_env();
    if !rhebok::backup::is_configured(cfg) {
        eprintln!("no backups configured");
        std::process::exit(2);
    }
    let mut failed = false;
    if let Err(e) = rhebok::backup::load_key(cfg) {
        eprintln!("key:    FAILED  {e:#}");
        failed = true;
    }
    for target in &cfg.backup.targets {
        match rhebok::backup::check_target(cfg, target) {
            Ok(place) => println!("{:<7} ok  {place}", format!("{target}:")),
            Err(e) => {
                eprintln!("{:<7} FAILED  {e:#}", format!("{target}:"));
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(feature = "backup")]
fn backup_auth_drive(cfg: &Config) -> Result<()> {
    let table = cfg
        .backup
        .drive
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no [backup.drive] table in your config"))?;
    let token = cfg.drive_token_path();
    let folder_id = rhebok::backup::remote::drive::authorize(
        &Config::expand_tilde_path(&table.client_secret_file),
        &token,
        &table.folder_id,
    )?;
    println!("authorized; token written to {}", token.display());
    if table.folder_id.is_empty() {
        println!(
            "created the Drive folder \"{}\". Add this under [backup.drive] in {}:",
            rhebok::backup::remote::drive::FOLDER_NAME,
            Config::config_path().display()
        );
        println!();
        println!("folder_id = \"{folder_id}\"");
        println!();
    }
    println!(
        "If backups stop working in about a week, the OAuth app is still in \"Testing\" status; \
         set it to \"In production\" in the Google Cloud console."
    );
    Ok(())
}

#[cfg(feature = "backup")]
fn backup_status(cfg: &Config) -> Result<()> {
    let db = Config::db_path();
    println!(
        "last backup: {}",
        rhebok::backup::describe_age(rhebok::backup::read_stamp(&db))
    );
    println!("targets:     {}", cfg.backup.targets.join(", "));
    println!("encrypted:   {}", cfg.backup.encrypt);
    println!("key:         {}", cfg.backup_key_path().display());
    if cfg.backup.drive.is_some() {
        match std::fs::metadata(cfg.drive_token_path()).and_then(|m| m.modified()) {
            Ok(t) => println!(
                "drive token: refreshed {}",
                rhebok::backup::describe_age(Some(t.into()))
            ),
            Err(_) => println!("drive token: missing; run `rhebok backup auth drive`"),
        }
    }
    for target in &cfg.backup.targets {
        let counted = rhebok::backup::remote_for(cfg, target).and_then(|r| r.list("manifest/"));
        match counted {
            Ok(objects) => println!(
                "{target}: {} generations",
                objects
                    .iter()
                    .filter(|o| o.key != rhebok::backup::manifest::Manifest::latest_key())
                    .count()
            ),
            Err(e) => println!("{target}: unreachable ({e:#})"),
        }
    }
    Ok(())
}

#[cfg(feature = "backup")]
fn backup_schedule(at: &str, uninstall: bool) -> Result<()> {
    let exe = rhebok::backup::schedule_executable(
        &rhebok::setup::Paths::from_env().bin,
        std::env::current_exe()?,
    );
    let explicit = std::env::var_os("RHEBOK_CONFIG").map(std::path::PathBuf::from);
    let line = rhebok::backup::cron_line(at, &exe, explicit.as_deref())?;
    let existing = std::process::Command::new("crontab")
        .arg("-l")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let body = rhebok::backup::crontab_with(&existing, (!uninstall).then_some(line.as_str()));
    let mut child = std::process::Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            anyhow::anyhow!("could not run `crontab`; is cron available on this machine? {e}")
        })?;
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin was piped")
            .write_all(body.as_bytes())?;
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow::anyhow!("`crontab -` exited with {status}"));
    }
    if uninstall {
        println!("removed the rhebok-backup crontab entry");
    } else {
        println!("installed: {line}");
        println!(
            "Cron does not run while the machine is asleep and does not catch up afterwards. \
             Pick an hour this machine is awake; `rhebok status` shows how long it has been since \
             the last successful backup."
        );
    }
    Ok(())
}

#[cfg(feature = "backup")]
fn run_restore(cfg: &Config, opts: rhebok::backup::RestoreOptions) -> Result<()> {
    if !rhebok::backup::is_configured(cfg) {
        eprintln!("no backups configured");
        std::process::exit(1);
    }
    let key = rhebok::backup::load_key(cfg)?;
    let target = &cfg.backup.targets[0];
    let remote = rhebok::backup::remote_for(cfg, target)?;
    let stats = rhebok::backup::restore(cfg, remote.as_ref(), key.as_ref(), &opts)?;
    for line in restore_report(&stats, target, opts.dry_run) {
        println!("{line}");
    }
    for line in restore_warnings(&stats) {
        eprintln!("{line}");
    }
    Ok(())
}

#[cfg(feature = "backup")]
fn restore_report(
    stats: &rhebok::backup::RestoreStats,
    target: &str,
    dry_run: bool,
) -> Vec<String> {
    use rhebok::backup::MemoryOutcome;
    let verb = if dry_run { "would restore" } else { "restored" };
    let mut lines = vec![format!(
        "backup written {} on {} ({} on {target})",
        stats.created_at, stats.host, stats.generation
    )];
    lines.push(format!(
        "{verb} {} files{}{}",
        stats.files,
        if stats.index_restored {
            " and the index"
        } else {
            ""
        },
        if stats.skipped_existing > 0 {
            format!(
                " ({} already present; --force overwrites them)",
                stats.skipped_existing
            )
        } else {
            String::new()
        }
    ));
    match &stats.memory {
        MemoryOutcome::NotInBackup => lines.push("this backup holds no memory store".into()),
        MemoryOutcome::Restored => lines.push(format!("{verb} the memory store")),
        MemoryOutcome::Replaced { previous } => lines.push(format!(
            "{verb} the memory store over the local one, which {} kept at {}",
            if dry_run { "would be" } else { "is" },
            previous.display()
        )),
        MemoryOutcome::KeptLocal => lines
            .push("kept the local memory store; --memories replaces it with the backup's".into()),
        MemoryOutcome::Incompatible(_) => {}
    }
    lines
}

#[cfg(feature = "backup")]
fn restore_warnings(stats: &rhebok::backup::RestoreStats) -> Vec<String> {
    let mut lines = Vec::new();
    if stats.newer_generations > 0 {
        lines.push(format!(
            "warning: the latest pointer names {}, but the target holds {} newer generation(s). \
             Check the date above; pass --generation to restore a specific one.",
            stats.generation, stats.newer_generations
        ));
    }
    if let rhebok::backup::MemoryOutcome::Incompatible(reason) = &stats.memory {
        lines.push(format!("warning: {reason}"));
    }
    lines
}

#[cfg(feature = "backup")]
fn append_backup_log(line: &str) {
    use std::io::Write;
    let path = Config::backup_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(
            f,
            "{}  {}",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            line.lines().collect::<Vec<_>>().join(" | ")
        );
    }
}

#[cfg(all(test, feature = "backup"))]
mod restore_output_tests {
    use super::{restore_report, restore_warnings};
    use rhebok::backup::{MemoryOutcome, RestoreStats};

    fn stats(memory: MemoryOutcome, newer_generations: usize) -> RestoreStats {
        RestoreStats {
            generation: "manifest/2026-01-02T03-04-05.006Z.json".into(),
            created_at: "2026-01-02T03:04:05.006Z".into(),
            host: "laptop".into(),
            newer_generations,
            files: 2,
            memory,
            ..RestoreStats::default()
        }
    }

    #[test]
    fn the_report_names_the_date_and_host_the_restored_manifest_was_written_on() {
        for dry_run in [true, false] {
            let lines = restore_report(&stats(MemoryOutcome::Restored, 0), "s3", dry_run);
            assert!(
                lines[0].contains("2026-01-02T03:04:05.006Z") && lines[0].contains("laptop"),
                "dry run {dry_run}: {lines:?}"
            );
        }
    }

    #[test]
    fn a_replaced_memory_store_is_reported_with_where_the_local_one_went() {
        let previous = std::path::PathBuf::from("/data/memory/db.before-restore-20260102T030405Z");
        let lines = restore_report(
            &stats(
                MemoryOutcome::Replaced {
                    previous: previous.clone(),
                },
                0,
            ),
            "s3",
            false,
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains(&previous.display().to_string())),
            "{lines:?}"
        );
    }

    #[test]
    fn an_older_generation_and_incompatible_memories_are_warned_about() {
        assert!(restore_warnings(&stats(MemoryOutcome::Restored, 0)).is_empty());
        let warnings = restore_warnings(&stats(
            MemoryOutcome::Incompatible("run `rhebok memory rebuild`".into()),
            3,
        ));
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("3 newer"), "{warnings:?}");
        assert!(
            warnings[1].contains("rhebok memory rebuild"),
            "{warnings:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit() -> Hit {
        Hit {
            chunk_id: "c1".into(),
            doc_id: "d1".into(),
            text: "the body of the note".into(),
            heading_path: "Notes / Setup".into(),
            uri: "notes/setup.md".into(),
            title: "Setup".into(),
            page_no: None,
            score: 0.5,
            relevance: 0.812,
            source_type: "markdown".into(),
            inbound: 0,
            lifecycle: rhebok::pack::status::Lifecycle::Current,
            last_used: None,
            memory: None,
            mount: None,
        }
    }

    #[test]
    fn a_styled_hit_frames_title_and_metadata_with_escapes() {
        let out = render_hit(&hit(), true);
        assert!(out.starts_with("\n\x1b[1mSetup\x1b[0m  (0.812)\n"));
        assert!(out.contains("  \x1b[2mNotes / Setup\x1b[0m\n"));
        assert!(out.contains("  \x1b[2mnotes/setup.md\x1b[0m\n"));
    }

    #[test]
    fn a_plain_hit_carries_no_escape_byte() {
        let out = render_hit(&hit(), false);
        assert!(!out.contains('\x1b'));
        assert!(out.starts_with("\nSetup  (0.812)\n"));
        assert!(out.contains("  Notes / Setup\n  notes/setup.md\n"));
    }

    #[test]
    fn an_empty_heading_is_skipped_in_both_styles() {
        let mut h = hit();
        h.heading_path = String::new();
        for styled in [true, false] {
            let out = render_hit(&h, styled);
            assert_eq!(out.matches("notes/setup.md").count(), 1);
        }
    }
}
