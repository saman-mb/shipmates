//! File deduplicated upstream issues for post-upgrade findings via `gh`.
//!
//! Dedupe is by an HTML comment marker carrying the fingerprint; a match becomes
//! a "still present" comment, a miss becomes a new issue. Bodies go to a
//! tempfile and through `--body-file`, labels are filtered to the ones that
//! exist, and home paths / private roots are sanitised before anything leaves
//! the machine. Filings are capped per run.

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::upgrade::classify::class_slug;
use crate::upgrade::types::Finding;

/// Options for [`file_bugs`]. `gh` defaults to `"gh"` (resolved through `PATH`);
/// unit tests point it at a fake script so they never touch the real `gh` or
/// mutate process env.
pub struct FileBugsOpts {
    pub repo: String,
    pub labels: Vec<String>,
    pub cap: usize,
    pub gh: String,
}

impl Default for FileBugsOpts {
    fn default() -> Self {
        Self {
            repo: "saman-mb/shipmates".to_string(),
            labels: Vec::new(),
            cap: 3,
            gh: "gh".to_string(),
        }
    }
}

/// What [`file_bugs`] did, by fingerprint.
#[derive(Debug, Default, Serialize)]
pub struct FileReport {
    pub filed: Vec<String>,
    pub commented: Vec<String>,
    pub skipped: Vec<String>,
}

/// File (or comment on) one issue per finding, deduped by fingerprint marker.
pub fn file_bugs(findings: &[Finding], opts: &FileBugsOpts) -> anyhow::Result<FileReport> {
    let mut report = FileReport::default();
    let mut filed_this_run = 0usize;

    let valid_labels = existing_labels(&opts.repo, &opts.gh, &opts.labels)?;

    for finding in findings {
        let marker = marker_for(finding);
        match existing_issue_for(&opts.repo, &opts.gh, &finding.fingerprint, &marker)? {
            Some(number) => {
                let body = comment_body(finding);
                run_gh(
                    &opts.gh,
                    &[
                        "issue",
                        "comment",
                        number.as_str(),
                        "--repo",
                        opts.repo.as_str(),
                    ],
                    &body,
                )?;
                report.commented.push(number);
            }
            None => {
                if filed_this_run >= opts.cap {
                    report.skipped.push(finding.fingerprint.clone());
                    continue;
                }
                let body = issue_body(finding);
                let title = issue_title(finding);
                let mut args: Vec<&str> = vec![
                    "issue",
                    "create",
                    "--repo",
                    opts.repo.as_str(),
                    "--title",
                    title.as_str(),
                ];
                for label in &valid_labels {
                    args.push("--label");
                    args.push(label.as_str());
                }
                run_gh(&opts.gh, &args, &body)?;
                report.filed.push(finding.fingerprint.clone());
                filed_this_run += 1;
            }
        }
    }

    Ok(report)
}

fn marker_for(finding: &Finding) -> String {
    format!("<!-- shipmates-fingerprint: {} -->", finding.fingerprint)
}

/// Write `body` to a tempfile and run `gh <args> --body-file <tmp>`, cleaning up
/// the tempfile when the subprocess returns. The file stays alive for the whole
/// run because it is only dropped when `gh` has read it.
fn run_gh(gh: &str, args: &[&str], body: &str) -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new().context("creating issue body tempfile")?;
    file.write_all(body.as_bytes())
        .context("writing issue body tempfile")?;
    file.flush().context("flushing issue body tempfile")?;

    let status = Command::new(gh)
        .args(args)
        .arg("--body-file")
        .arg(file.path())
        .status()
        .with_context(|| format!("running {gh} {}", args.join(" ")))?;
    if !status.success() {
        anyhow::bail!("{gh} {} exited {}", args.join(" "), status);
    }
    Ok(())
}

/// The open issue whose body carries `marker`, if any.
///
/// The `--search` query is the contract's `"<fingerprint> in:body"`; the body is
/// then re-checked for the exact marker, because GitHub's search tokenisation is
/// lossy on `|` and `/`.
fn existing_issue_for(
    repo: &str,
    gh: &str,
    fingerprint: &str,
    marker: &str,
) -> anyhow::Result<Option<String>> {
    let search = format!("{fingerprint} in:body");
    let output = Command::new(gh)
        .args([
            "issue",
            "list",
            "--repo",
            repo,
            "--state",
            "open",
            "--search",
            &search,
            "--json",
            "number,url,body",
        ])
        .output()
        .with_context(|| format!("running {gh} issue list"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "{gh} issue list failed: {} — {}",
            output.status,
            stderr.trim()
        );
    }

    #[derive(Debug, Deserialize)]
    struct Issue {
        number: u64,
        body: Option<String>,
    }
    let issues: Vec<Issue> =
        serde_json::from_slice(&output.stdout).context("parsing `gh issue list` output")?;
    for issue in issues {
        if issue
            .body
            .as_deref()
            .is_some_and(|body| body.contains(marker))
        {
            return Ok(Some(issue.number.to_string()));
        }
    }
    Ok(None)
}

/// Labels from `opts.labels` that actually exist on the repo. An unqueryable
/// label list degrades to no labels rather than aborting a filing run.
fn existing_labels(repo: &str, gh: &str, labels: &[String]) -> anyhow::Result<Vec<String>> {
    if labels.is_empty() {
        return Ok(Vec::new());
    }
    let output = Command::new(gh)
        .args(["label", "list", "--repo", repo, "--json", "name"])
        .output()
        .with_context(|| format!("running {gh} label list"))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    #[derive(Debug, Deserialize)]
    struct Label {
        name: String,
    }
    let existing: Vec<Label> = serde_json::from_slice(&output.stdout).unwrap_or_default();
    let existing: HashSet<String> = existing.into_iter().map(|label| label.name).collect();
    Ok(labels
        .iter()
        .filter(|label| existing.contains(*label))
        .cloned()
        .collect())
}

fn issue_title(finding: &Finding) -> String {
    format!(
        "[upgrade] {} on {}",
        class_slug(finding.class),
        finding.harness
    )
}

fn issue_body(finding: &Finding) -> String {
    format!(
        "{}\n\n**Harness:** {}\n**Root:** {}\n**Detail:** {}\n\n{}\n",
        issue_title(finding),
        finding.harness,
        sanitize_root(&finding.root),
        sanitize_text(&finding.detail),
        marker_for(finding),
    )
}

fn comment_body(finding: &Finding) -> String {
    format!(
        "Still present on shipmates v{} ({}).\n\n**Detail:** {}\n\n{}\n",
        env!("CARGO_PKG_VERSION"),
        class_slug(finding.class),
        sanitize_text(&finding.detail),
        marker_for(finding),
    )
}

/// Replace the home directory prefix with `~`; anything else absolute becomes
/// `<project-root>` (a private project root must never leak).
fn sanitize_root(root: &str) -> String {
    let path = Path::new(root);
    if let Some(home) = home::home_dir()
        && let Ok(relative) = path.strip_prefix(&home)
    {
        return format!("~/{}", relative.to_string_lossy());
    }
    "<project-root>".to_string()
}

/// Replace any occurrence of the home directory with `~` in free text.
fn sanitize_text(text: &str) -> String {
    match home::home_dir() {
        Some(home) => text.replace(&home.to_string_lossy().to_string(), "~"),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upgrade::classify::fingerprint;
    use crate::upgrade::types::FindingClass;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn finding(harness: &str, class: FindingClass) -> Finding {
        let home = home::home_dir()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/home/me".to_string());
        Finding {
            class,
            harness: harness.to_string(),
            root: format!("{home}/work/project"),
            detail: "post-upgrade audit found problems".to_string(),
            fingerprint: fingerprint(harness, class),
        }
    }

    /// A fake `gh` that logs its args (and any `--body-file` contents) and prints
    /// canned JSON for `issue list` / `label list`. The canned JSON is written to
    /// files whose paths are baked into the script, so no process env is touched.
    fn write_fake_gh(dir: &Path, list_json: &str, label_json: &str) -> PathBuf {
        let gh = dir.join("fake-gh");
        let log = dir.join("gh.log");
        let list_file = dir.join("list.json");
        let label_file = dir.join("labels.json");
        std::fs::write(&list_file, list_json).unwrap();
        std::fs::write(&label_file, label_json).unwrap();
        let script = format!(
            "#!/bin/sh\nLOG='{}'\nLIST='{}'\nLABELS='{}'\necho \"ARGS: $*\" >> \"$LOG\"\nprev=\"\"\nfor a in \"$@\"; do\n  if [ \"$prev\" = \"--body-file\" ]; then\n    echo 'BODY:' >> \"$LOG\"\n    cat \"$a\" >> \"$LOG\"\n    echo >> \"$LOG\"\n  fi\n  prev=\"$a\"\ndone\nif [ \"$1\" = \"issue\" ] && [ \"$2\" = \"list\" ]; then cat \"$LIST\"; fi\nif [ \"$1\" = \"label\" ] && [ \"$2\" = \"list\" ]; then cat \"$LABELS\"; fi\nexit 0\n",
            log.display(),
            list_file.display(),
            label_file.display(),
        );
        std::fs::write(&gh, script).unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        gh
    }

    fn opts(gh: &Path, cap: usize, labels: Vec<String>) -> FileBugsOpts {
        FileBugsOpts {
            repo: "saman-mb/shipmates".to_string(),
            labels,
            cap,
            gh: gh.to_string_lossy().into_owned(),
        }
    }

    fn log(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("gh.log")).unwrap_or_default()
    }

    #[test]
    fn match_comments_instead_of_creating() {
        let dir = tempfile::tempdir().unwrap();
        let finding = finding("claude-code", FindingClass::ToolFailure);
        let marker = marker_for(&finding);
        let list_json = format!(
            r#"[{{"number":42,"url":"https://github.com/saman-mb/shipmates/issues/42","body":"{marker}"}}]"#
        );
        let gh = write_fake_gh(dir.path(), &list_json, "[]");

        let report = file_bugs(&[finding], &opts(&gh, 3, vec![])).unwrap();
        assert!(report.filed.is_empty());
        assert_eq!(report.commented, vec!["42".to_string()]);
        let logged = log(dir.path());
        assert!(logged.contains("issue comment 42"), "{logged}");
        assert!(!logged.contains("issue create"), "{logged}");
    }

    #[test]
    fn no_match_creates() {
        let dir = tempfile::tempdir().unwrap();
        let finding = finding("claude-code", FindingClass::UnrepairableDrift);
        let fp = finding.fingerprint.clone();
        let gh = write_fake_gh(dir.path(), "[]", "[]");

        let report = file_bugs(&[finding], &opts(&gh, 3, vec![])).unwrap();
        assert_eq!(report.filed, vec![fp]);
        assert!(report.commented.is_empty());
        let logged = log(dir.path());
        assert!(logged.contains("issue create"), "{logged}");
        assert!(!logged.contains("issue comment"), "{logged}");
    }

    #[test]
    fn cap_limits_filings_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let first = finding("claude-code", FindingClass::ToolFailure);
        let second = finding("opencode", FindingClass::UnrepairableDrift);
        let gh = write_fake_gh(dir.path(), "[]", "[]");

        let report = file_bugs(&[first, second], &opts(&gh, 1, vec![])).unwrap();
        assert_eq!(report.filed.len(), 1);
        assert_eq!(report.skipped.len(), 1);
    }

    #[test]
    fn labels_are_filtered_to_existing() {
        let dir = tempfile::tempdir().unwrap();
        let finding = finding("claude-code", FindingClass::UnrepairableDrift);
        let gh = write_fake_gh(dir.path(), "[]", r#"[{"name":"bug"},{"name":"shipmates"}]"#);

        file_bugs(
            &[finding],
            &opts(&gh, 3, vec!["bug".to_string(), "nonexistent".to_string()]),
        )
        .unwrap();
        let logged = log(dir.path());
        assert!(logged.contains("--label bug"), "{logged}");
        assert!(!logged.contains("nonexistent"), "{logged}");
    }

    #[test]
    fn home_paths_are_sanitised_to_tilde() {
        let dir = tempfile::tempdir().unwrap();
        let finding = finding("claude-code", FindingClass::UnrepairableDrift);
        let gh = write_fake_gh(dir.path(), "[]", "[]");

        file_bugs(&[finding], &opts(&gh, 3, vec![])).unwrap();
        let logged = log(dir.path());
        if let Some(home) = home::home_dir() {
            let home_str = home.to_string_lossy();
            assert!(!logged.contains(home_str.as_ref()), "home leaked: {logged}");
            assert!(logged.contains('~'), "expected ~: {logged}");
        } else {
            assert!(logged.contains("<project-root>"), "{logged}");
        }
    }
}
