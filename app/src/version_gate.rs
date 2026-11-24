use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CLI_LIMIT: Duration = Duration::from_secs(3);
const CLI_POLL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    Ok,
    Missing { fix: String },
    Unanswered { fix: String },
    Mismatch { cli: String, app: String },
}

pub fn check(cli: &Path, own: &str) -> Gate {
    check_within(cli, own, CLI_LIMIT)
}

fn check_within(cli: &Path, own: &str, limit: Duration) -> Gate {
    match cli_version(cli, limit) {
        Answer::Version(version) if version == own => Gate::Ok,
        Answer::Version(version) => Gate::Mismatch {
            cli: version,
            app: own.to_string(),
        },
        Answer::Silent => unanswered(cli, &format!("did not answer within {}", waited(limit))),
        Answer::Killed => unanswered(cli, "was stopped before it answered"),
        Answer::Unusable => Gate::Missing {
            fix: format!(
                "rhebok is not installed at {}; run the rhebok installer, then open Rhebok again",
                cli.display()
            ),
        },
    }
}

fn unanswered(cli: &Path, what_happened: &str) -> Gate {
    Gate::Unanswered {
        fix: format!(
            "rhebok at {0} {what_happened}; open Rhebok again, and if it keeps happening, run \"{0}\" --version in a terminal",
            cli.display()
        ),
    }
}

fn waited(limit: Duration) -> String {
    match limit.as_millis() {
        1000 => "1 second".to_string(),
        ms if ms % 1000 == 0 => format!("{} seconds", ms / 1000),
        ms => format!("{ms} ms"),
    }
}

enum Answer {
    Version(String),
    Silent,
    Killed,
    Unusable,
}

fn cli_version(cli: &Path, limit: Duration) -> Answer {
    let Ok(mut child) = Command::new(cli)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Answer::Unusable;
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.signal().is_some() => return Answer::Killed,
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Answer::Silent;
            }
            Ok(None) => std::thread::sleep(CLI_POLL),
            Err(_) => return Answer::Unusable,
        }
    }
    child
        .wait_with_output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| reported_version(&text))
        .map_or(Answer::Unusable, Answer::Version)
}

fn reported_version(text: &str) -> Option<String> {
    let mut words = text.split_whitespace();
    match (words.next(), words.next(), words.next()) {
        (Some("rhebok"), Some(version), None) => Some(version.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("rhebok");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn a_missing_cli_is_a_fix_not_a_crash() {
        let tmp = tempfile::tempdir().unwrap();
        match check(&tmp.path().join("nope"), "0.2.0") {
            Gate::Missing { fix } => assert!(fix.contains("install")),
            other => panic!("expected Missing, got {other:?}"),
        }
    }

    #[test]
    fn a_mismatched_cli_shows_only_the_update_notice() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = script(tmp.path(), "echo 'rhebok 0.1.9'");
        assert_eq!(
            check(&cli, "0.2.0"),
            Gate::Mismatch {
                cli: "0.1.9".into(),
                app: "0.2.0".into()
            }
        );
    }

    #[test]
    fn a_hung_cli_is_reported_as_unanswered_within_the_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = script(tmp.path(), "exec sleep 30");
        let started = std::time::Instant::now();
        let gate = check_within(&cli, "0.2.0", Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            gate,
            Gate::Unanswered {
                fix: format!(
                    "rhebok at {0} did not answer within 200 ms; open Rhebok again, and if it keeps happening, run \"{0}\" --version in a terminal",
                    cli.display()
                )
            }
        );
    }

    #[test]
    fn the_advice_quotes_a_path_with_a_space_so_it_can_be_pasted() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("Application Support");
        std::fs::create_dir(&support).unwrap();
        let cli = script(&support, "exec sleep 30");
        let Gate::Unanswered { fix } = check_within(&cli, "0.2.0", Duration::from_millis(200))
        else {
            panic!("a hung CLI is unanswered");
        };
        assert!(
            fix.ends_with(&format!(
                "run \"{}\" --version in a terminal",
                cli.display()
            )),
            "{fix}"
        );
    }

    #[test]
    fn a_cli_killed_before_it_answers_is_not_called_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let support = tmp.path().join("Application Support");
        std::fs::create_dir(&support).unwrap();
        let cli = script(&support, "kill -9 $$");
        assert_eq!(
            check(&cli, "0.2.0"),
            Gate::Unanswered {
                fix: format!(
                    "rhebok at {0} was stopped before it answered; open Rhebok again, and if it keeps happening, run \"{0}\" --version in a terminal",
                    cli.display()
                )
            }
        );
    }

    #[test]
    fn the_shipped_limit_is_named_in_seconds() {
        assert_eq!(waited(CLI_LIMIT), "3 seconds");
        assert_eq!(waited(Duration::from_secs(1)), "1 second");
    }

    #[test]
    fn the_gate_waits_a_few_seconds_for_the_cli() {
        assert_eq!(CLI_LIMIT, Duration::from_secs(3));
    }

    #[test]
    fn matching_versions_pass() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = script(tmp.path(), "echo 'rhebok 0.2.0'");
        assert_eq!(check(&cli, "0.2.0"), Gate::Ok);
    }

    #[test]
    fn a_cli_that_fails_or_prints_nonsense_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let failing = script(tmp.path(), "exit 3");
        assert!(matches!(check(&failing, "0.2.0"), Gate::Missing { .. }));
        let nonsense = script(tmp.path(), "echo hello");
        assert!(matches!(check(&nonsense, "0.2.0"), Gate::Missing { .. }));
    }
}
