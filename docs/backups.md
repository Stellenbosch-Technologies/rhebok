# Backups


`rhebok backup` copies these to Amazon S3, Google Drive, or both:

| What | Why it is worth keeping |
|---|---|
| `config.toml` | Your tuned thresholds, tiers and weights. Re-deriving them means re-running `rhebok bench`. |
| `golden.toml` | Your bench set, written by hand. Nothing regenerates it. |
| The memory store | Your lessons, facts and episodes. Nothing can rebuild them from your files. |
| The index | Rebuildable, but a large corpus takes hours of embedding to rebuild. Only with `include_index = true`, the default. |

The memory store (`memory/db` and its pack, beside the index) is always
included when it exists. rhebok copies it under the memory store's own lock,
so it never copies a half-written memory. If another rhebok process holds
that lock for more than a few seconds, the backup fails and writes nothing,
rather than writing a backup without your memories.

Your notes, your `sources` folders and your Claude Code session transcripts
are **not** backed up. Treat the backup as a warm-start
cache plus your tuning, not as protection for your data. It matters on a new
machine: `rhebok index` removes every document whose source file it cannot
find, so an index restored onto a machine that lacks the original files is
pruned away by the next indexing run. Back up your notes the way you already do
(a git remote, Time Machine), and use this to skip the re-embedding.

Uploads are content-addressed. After the first run, only what changed is sent,
so a daily backup of an unchanged setup transfers almost nothing.

## Encryption

File contents are encrypted on your machine before upload, and encryption is
on by default. Generate the key first:

```
rhebok backup init
```

It prints the key once and asks you to store a copy somewhere other than this
machine. **If you lose the key, the backups are unrecoverable.** There is no
recovery path, by design. Set `encrypt = false` under `[backup]` if you would
rather not carry that risk.

What encryption does and does not hide from your storage provider:

- **File contents** are encrypted. Each backup also has a manifest, the list
  of what it holds, and with a key configured the manifest carries an
  HMAC-SHA256 tag computed under a key derived from your backup key. A
  restore checks that tag, so nobody can change a manifest without the
  restore noticing.
- **The manifest itself is not encrypted.** Your provider can read the
  host name, file names, sizes, hashes and document counts in it.
- **Blobs are named by the SHA-256 of their plain content**, and encryption
  is deterministic, so your provider can tell when two files are identical.
- **Someone with write access to your bucket or Drive folder** cannot forge a
  backup, but can point `manifest/latest.json` at an older genuine one.
  Every restore prints when and where the backup it restores was written;
  check that date.

Without a key (`encrypt = false`), a manifest has no tag, and a restore
trusts whatever the target holds. Encryption is what protects you from
someone who can write to your backup target.

## Amazon S3

```toml
[backup]
enabled = true
targets = ["s3"]

[backup.s3]
bucket = "my-rhebok-backups"
region = "eu-west-1"
profile = "default"
```

Credentials come from the named profile in `~/.aws/credentials`, **not** from
environment variables. That is deliberate: cron runs with almost no
environment, so an `AWS_ACCESS_KEY_ID` exported in your shell does not exist
when the backup actually runs. Objects go under `prefix` (default `rhebok/`)
with storage class `storage_class` (default `STANDARD_IA`).

## Google Drive

Drive needs an OAuth client of your own:

1. In the Google Cloud console, create a project and enable the Drive API.
2. Under **APIs & Services > OAuth consent screen**, set the publishing status
   to **In production**. Do not leave it in *Testing*: Google expires refresh
   tokens for apps in Testing after 7 days, and your backups would stop about
   a week later with no other symptom.
3. Under **Credentials**, create an OAuth client ID of type *Desktop app* and
   download the JSON.

```toml
[backup]
enabled = true
targets = ["drive"]

[backup.drive]
client_secret_file = "/path/to/drive-client.json"
```

Keep the downloaded JSON somewhere private; the folder that holds
`config.toml` (`rhebok config path` prints the file's path) is a good place.

Then authorize once, interactively:

```
rhebok backup auth drive
```

It opens a browser for consent, creates a folder named `rhebok backups` in your
Drive, and prints a `folder_id = "..."` line to add under `[backup.drive]`.
Let it create the folder. `rhebok` asks only for the `drive.file` scope, which
lets it see nothing but files it created itself: the rest of your Drive stays
invisible to it, and so does any folder you make by hand. A backup run never
opens a browser. If the token cannot be refreshed, the run fails and names the
command to re-run.

## Running it

Check the setup before trusting it. `check` clears the AWS and Google
environment variables first, so a configuration that works only in your shell
fails here instead of at 3am:

```
rhebok backup check
rhebok backup
```

Then schedule it:

```
rhebok backup schedule --at "0 13 * * *"
```

The schedule is a crontab line. It runs the installed `rhebok` program by its
full path, quoted, so a path with spaces (the default on macOS) works.
`--at` takes five cron fields or one of `@reboot`, `@yearly`, `@annually`,
`@monthly`, `@weekly`, `@daily`, `@midnight` and `@hourly`; anything else is
refused. A schedule written unquoted by a version before 0.2.0 is quoted by
`rhebok install`, which `rhebok update` runs, with its time, program and
config kept. A commented-out line is not a schedule, and install leaves it
alone. If install cannot read a live line, it warns and leaves it alone; run
`rhebok backup schedule` again to replace it.

Pick an hour the machine is awake. Cron does not run while a laptop is asleep
and does not catch up when it wakes, so a 3am job on a closed laptop never
fires. `rhebok status` shows how long it has been since every target last
succeeded, which is how you notice.

`rhebok backup schedule --uninstall` removes the entry, and running `schedule`
again replaces it rather than adding a second one. `rhebok backup --help` lists
every subcommand. Each run writes one line to `db.backup.log` beside the index
and exits 0 when it backed up, 1 when something failed, and 2 when there was
nothing to do (no backup configured, or another run in progress).

Each run keeps 30 generations of your files and the last 2 index snapshots
(`keep_generations` and `keep_index`). The index gets a shorter history on
purpose: it is large, it never dedupes, and it is a cache. A memory snapshot
is kept for as long as a generation that names it is kept.

## Restoring

```
rhebok backup status         # last success, and the generations each target holds
rhebok restore --dry-run     # say what would be restored, and change nothing
rhebok restore               # config.toml, golden.toml, and the memory store if you have none
rhebok restore --index       # the index as well, if it is compatible
```

A restore reads from the first target in `targets`, from the newest
generation unless you pass `--generation <key>`. Every restore, a dry run
included, first prints when and on which host that backup was written. If
`manifest/latest.json` names a generation older than others on the target, it
warns you and says how many are newer.

A dry run downloads, decrypts and hash-checks only what a real restore would
write: the config files that are missing locally (all of them with
`--force`) and the memory snapshot when it would be restored. A config file
that would be skipped is not downloaded. To check the memory snapshot, a dry
run unpacks it into a temporary `memory.restore.staging` directory beside
the `memory` directory, checks that it holds a memory store, and then deletes
that directory. A real restore uses the same directory, so do not start a dry run
while a restore is running. The index snapshot is never
downloaded by a dry run: with `--index` it checks only that the manifest's
index is compatible with your settings, and the report does not mention the
index. A dry run that passes therefore does not prove the index snapshot on
the target is intact.

What a restore writes:

- **`config.toml` and `golden.toml`**, when they do not exist locally.
  `--force` overwrites them. A restore never writes any other config file a
  manifest names, such as `env`.
- **The memory store**, when you have none. An existing one is kept, and the
  output says so. `--memories` replaces it with the backup's; your local
  store is renamed, never deleted, to `memory/db.before-restore-<time>`, and
  the output prints that path. `--force` never touches memories.
- **The index**, only with `--index`. It is refused outright if the backup's
  index was built with a different embedding model, width or chunk size:
  those embeddings are not comparable, so the restored index would silently
  return nonsense. Restore without `--index` and run `rhebok index --reindex`
  instead.

The memory snapshot records the embedding model and dimensions it was written
with. If they differ from the `[embed]` settings in effect after the restore,
the memories are not restored, the rest of the restore goes ahead, and a
warning explains how to get them back: set `[embed] model` and `dimensions`
to the backup's, run `rhebok restore --memories`, then put your settings back
and run `rhebok memory rebuild`.

With a backup key configured, a restore refuses:

- a manifest whose tag does not match, because it was changed after it was
  written. Nothing overrides this.
- a manifest stored under another generation's name, which is how an old
  backup is passed off as a newer one.
- an encrypted manifest with no tag. Every backup made before rhebok 0.2.0
  has none. If the backup is your own and you trust the target, run
  `rhebok restore --allow-unauthenticated-manifest`; the next
  `rhebok backup` writes an authenticated one.
- a manifest that says it is not encrypted. `--allow-unencrypted-manifest`
  accepts it, but only use it for a backup you made before you turned
  encryption on, on a target nobody else can write to: anyone who can write
  there can make such a manifest and choose every byte it restores, your
  memories included.

Backups made by rhebok 0.2.0 or later use manifest version 2. An older rhebok
refuses to restore them and says to upgrade.

Never backed up: your encryption key, your Drive token, the `env` file with
an embedding token, the index lock, the progress file, the logs, and the half-built (`db.new`) and previous (`db.old`)
index generations.
