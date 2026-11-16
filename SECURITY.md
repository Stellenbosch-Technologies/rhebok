# Security policy

## Reporting a vulnerability

Please do not report security problems in a public issue.

Report them privately through GitHub: open the repository's **Security** tab
and click **Report a vulnerability**, or go straight to
<https://github.com/Stellenbosch-Technologies/rhebok/security/advisories/new>.
Only you and the maintainers can see the report.

Please include:

- what an attacker can do, and what they need first (for example a web page
  you visit, write access to your backup bucket, or a file in your index);
- the steps to reproduce it, or a proof of concept;
- the rhebok version (`rhebok --version`), your operating system, and which
  agent you use;
- any fix or mitigation you have in mind.

We will reply in the advisory, work on a fix with you there, and credit you
in the advisory unless you would rather we did not. This is a small project,
so we cannot promise a response time. Please give us a reasonable chance to
release a fix before you publish details.

## What is in scope

- the `rhebok` program, including its prompt hook, the prehook mod, the MCP
  server and the local dashboard;
- the installer (`scripts/install.sh`), `rhebok install` and `rhebok update`;
- backups and restores;
- the release pipeline in `.github/workflows`.

Problems in Ollama, Claude Code, the other agents, or rhebok's dependencies
belong with those projects, unless rhebok uses them in an unsafe way.

[docs/security-model.md](docs/security-model.md) describes what rhebok trusts,
what can reach your agent's prompts, and how rhebok keeps retrieved text in
its place. Behaviour that it describes as expected, such as a note you indexed
containing misleading text, is not a vulnerability by itself.

## Supported versions

Only the latest release gets security fixes. Run `rhebok update` to install
it.
