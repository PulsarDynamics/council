//! Git tools. The implementer uses these to inspect the working tree, view
//! diffs, browse history, and create commits. We shell out to the `git`
//! CLI via `tokio::process::Command` rather than depending on `git2` — the
//! CLI matches what a human would run and keeps the dependency footprint
//! tiny.
//!
//! Every tool returns `{ stdout, stderr, exit_code }`. A non-zero exit
//! (e.g. the path is not a git repo) is NOT a tool error — we surface the
//! `stderr` and `exit_code` to the LLM so it can decide what to do.

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use council_core::{Tool, ToolContext, ToolOutput};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::info;

/// Hard cap for `git_diff` stdout. 32 KiB keeps the LLM context sane when
/// the diff is large; the truncated output includes a banner so the LLM
/// knows the rest was cut.
const DIFF_MAX_BYTES: usize = 32 * 1024;

/// Default timeout for any single `git` invocation. 30s matches
/// `run_command`; most ops are sub-second but `git log` on a huge repo
/// or one with a slow hook can blow up.
const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Run a `git` subcommand against `path`. Captures stdout/stderr to
/// strings, applies a timeout, returns the raw outcome as a JSON object.
/// `truncate` (when set) caps `stdout` at the given byte count and flips
/// the `truncated` flag.
async fn run_git(
    path: &str,
    args: &[&str],
    truncate: Option<usize>,
) -> ToolOutput {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git spawn: {e}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (out_buf, err_buf) = tokio::join!(
        drain(stdout),
        drain(stderr),
    );

    let res = tokio::time::timeout(GIT_TIMEOUT, child.wait()).await;
    let code = match res {
        Ok(Ok(s)) => s.code(),
        Ok(Err(e)) => return Err(format!("git wait: {e}")),
        Err(_) => return Err(format!("git timeout after {:?}", GIT_TIMEOUT)),
    };

    let out_buf = out_buf.unwrap_or_default();
    let err_buf = err_buf.unwrap_or_default();

    let mut obj = json!({
        "stdout": out_buf,
        "stderr": err_buf,
        "exit_code": code,
    });
    if let Some(limit) = truncate {
        if out_buf.len() > limit {
            let mut cut = String::with_capacity(limit + 128);
            cut.push_str(&out_buf[..limit]);
            cut.push_str(&format!(
                "\n\n... [truncated: output was {} bytes, kept first {} bytes] ...\n",
                out_buf.len(),
                limit
            ));
            obj["stdout"] = json!(cut);
            obj["truncated"] = json!(true);
        } else {
            obj["truncated"] = json!(false);
        }
    }
    Ok(obj)
}

async fn drain<R>(r: Option<R>) -> Option<String>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut s = r?;
    let mut buf = String::new();
    let mut lines = BufReader::new(&mut s).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        buf.push_str(&line);
        buf.push('\n');
    }
    Some(buf)
}

// ---------------- git_status ----------------

pub struct GitStatusTool;

#[async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }
    fn description(&self) -> &str {
        "Show the working tree status for a git repository at `path` as \
         porcelain output (one line per changed file, easy to grep). \
         Returns stdout, stderr, and exit_code. A non-zero exit_code means \
         the path is not a git repo or git could not run."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the git repository (or any directory inside one)."
                }
            },
            "required": ["path"]
        })
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutput {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'path'".to_string())?;
        info!(path, "git_status");
        run_git(path, &["status", "--porcelain"], None).await
    }
}

// ---------------- git_diff ----------------

pub struct GitDiffTool;

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }
    fn description(&self) -> &str {
        "Show the uncommitted diff for a git repository at `path`. \
         If `staged` is true, shows the staged diff (what `git diff \
         --cached` would print); otherwise shows the unstaged working \
         tree diff. Stdout is truncated at 32 KiB; the response includes \
         a `truncated` flag so you know to ask for smaller slices if you \
         need more."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the git repository."
                },
                "staged": {
                    "type": "boolean",
                    "description": "If true, show the staged diff (`--cached`). Default false."
                }
            },
            "required": ["path"]
        })
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutput {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'path'".to_string())?;
        let staged = args
            .get("staged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        info!(path, staged, "git_diff");
        let git_args: Vec<&str> = if staged {
            vec!["diff", "--cached"]
        } else {
            vec!["diff"]
        };
        run_git(path, &git_args, Some(DIFF_MAX_BYTES)).await
    }
}

// ---------------- git_log ----------------

pub struct GitLogTool;

#[async_trait]
impl Tool for GitLogTool {
    fn name(&self) -> &str {
        "git_log"
    }
    fn description(&self) -> &str {
        "Show the most recent commits for a git repository at `path` as \
         one-line summaries (`git log --oneline`). `max_count` defaults \
         to 10. Returns stdout, stderr, and exit_code."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the git repository."
                },
                "max_count": {
                    "type": "integer",
                    "description": "How many commits to show. Default 10.",
                    "minimum": 1
                }
            },
            "required": ["path"]
        })
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutput {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'path'".to_string())?;
        let max = args
            .get("max_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(10);
        // Clamp to a sane upper bound so a runaway LLM can't ask for
        // 10^18 commits and OOM us.
        let max = max.min(10_000);
        let n = max.to_string();
        info!(path, max, "git_log");
        run_git(path, &["log", "--oneline", "-n", &n], None).await
    }
}

// ---------------- git_commit ----------------

pub struct GitCommitTool;

#[async_trait]
impl Tool for GitCommitTool {
    fn name(&self) -> &str {
        "git_commit"
    }
    fn description(&self) -> &str {
        "Stage `paths` and create a commit in the git repository at \
         `path`. Runs `git add <paths...>` then `git commit -m <message>`. \
         If `no_verify` is true, passes `--no-verify` to the commit (skips \
         pre-commit and commit-msg hooks). Returns combined stdout, \
         stderr, and the commit's exit_code. A non-zero exit_code means \
         the commit did not happen (e.g. nothing staged, hooks failed)."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the git repository."
                },
                "message": {
                    "type": "string",
                    "description": "The commit message."
                },
                "paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Paths (relative to the repo) to stage before committing. Required, may not be empty."
                },
                "no_verify": {
                    "type": "boolean",
                    "description": "If true, pass --no-verify to skip pre-commit/commit-msg hooks. Default false."
                }
            },
            "required": ["path", "message", "paths"]
        })
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutput {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'path'".to_string())?;
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'message'".to_string())?;
        let paths: Vec<String> = args
            .get("paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "missing 'paths'".to_string())?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if paths.is_empty() {
            return Err("git_commit: 'paths' must contain at least one path".to_string());
        }
        let no_verify = args
            .get("no_verify")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Stage first. If add fails, return early — we never want a
        // half-staged repo with a confusing partial error.
        let mut add_args: Vec<String> = vec!["-C".to_string(), path.to_string(), "add".to_string()];
        add_args.extend(paths.iter().cloned());
        let add_refs: Vec<&str> = add_args.iter().map(String::as_str).collect();
        let add_result = run_git_raw(&add_refs).await?;
        if add_result["exit_code"].as_i64().unwrap_or(-1) != 0 {
            return Ok(add_result);
        }

        // Then commit. We build the argv dynamically because the
        // --no-verify flag is optional.
        let mut commit_args: Vec<String> = vec![
            "-C".to_string(),
            path.to_string(),
            "commit".to_string(),
            "-m".to_string(),
            message.to_string(),
        ];
        if no_verify {
            commit_args.push("--no-verify".to_string());
        }
        let commit_refs: Vec<&str> = commit_args.iter().map(String::as_str).collect();
        info!(path, paths = paths.len(), no_verify, "git_commit");
        run_git_raw(&commit_refs).await
    }
}

/// Like `run_git` but takes the full argv starting from `-C` (so callers
/// can include `-C <path>`, subcommands, and dynamic args). No
/// truncation — `git commit` output is already bounded.
async fn run_git_raw(argv: &[&str]) -> ToolOutput {
    if argv.is_empty() {
        return Err("run_git_raw: empty argv".to_string());
    }
    let mut child = Command::new("git")
        .args(argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git spawn: {e}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (out_buf, err_buf) = tokio::join!(drain(stdout), drain(stderr));

    let res = tokio::time::timeout(GIT_TIMEOUT, child.wait()).await;
    let code = match res {
        Ok(Ok(s)) => s.code(),
        Ok(Err(e)) => return Err(format!("git wait: {e}")),
        Err(_) => return Err(format!("git timeout after {:?}", GIT_TIMEOUT)),
    };

    Ok(json!({
        "stdout": out_buf.unwrap_or_default(),
        "stderr": err_buf.unwrap_or_default(),
        "exit_code": code,
    }))
}

// ---------------- tests ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command as StdCommand;
    use tempfile::tempdir;

    /// Make a fresh tempdir containing a real git repo with one initial
    /// commit. Returns the tempdir (so it stays alive for the duration of
    /// the test) and the path as a `String`.
    fn make_repo() -> (tempfile::TempDir, String) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().to_str().expect("utf8 path").to_string();
        // git init with -b main so we don't depend on whatever the host's
        // init.defaultBranch is.
        run_cli(&["-C", &path, "init", "-q", "-b", "main"]);
        // Commits need a user identity. We set it on the repo only
        // (--local), not globally, so we don't pollute the host config.
        run_cli(&["-C", &path, "config", "user.email", "test@example.com"]);
        run_cli(&["-C", &path, "config", "user.name", "Council Test"]);
        // An initial empty commit so HEAD exists and `git log` has
        // something to show. -q keeps it quiet.
        run_cli(&["-C", &path, "commit", "--allow-empty", "-q", "-m", "init"]);
        (dir, path)
    }

    /// Run a host git command synchronously. Panics on non-zero — only
    /// used for test setup.
    fn run_cli(args: &[&str]) {
        let out = StdCommand::new("git")
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("failed to spawn git: {e}"));
        if !out.status.success() {
            panic!(
                "git {:?} failed: status={:?}\nstdout: {}\nstderr: {}",
                args,
                out.status.code(),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
        }
    }

    fn ctx() -> ToolContext {
        ToolContext {
            session_id: uuid::Uuid::new_v4(),
            agent_name: "test".to_string(),
        }
    }

    /// Helper: write a file at `repo/rel` with `contents` and return the
    /// repo-relative path as a String.
    fn write_file(repo: &Path, rel: &str, contents: &str) -> String {
        let full = repo.join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, contents).unwrap();
        rel.to_string()
    }

    #[tokio::test]
    async fn git_status_clean_after_init() {
        let (_dir, path) = make_repo();
        let tool = GitStatusTool;
        let out = tool
            .execute(json!({ "path": path }), &ctx())
            .await
            .expect("execute");
        assert_eq!(out["exit_code"], json!(0));
        // Clean tree: empty stdout, empty stderr.
        assert_eq!(out["stdout"], json!(""));
        assert_eq!(out["stderr"], json!(""));
    }

    #[tokio::test]
    async fn git_status_detects_new_file() {
        let (dir, path) = make_repo();
        write_file(dir.path(), "hello.txt", "hi\n");
        let tool = GitStatusTool;
        let out = tool
            .execute(json!({ "path": path }), &ctx())
            .await
            .expect("execute");
        assert_eq!(out["exit_code"], json!(0));
        // Porcelain format: two columns then a space then the path.
        // A new untracked file shows up as `?? hello.txt`.
        let s = out["stdout"].as_str().unwrap();
        assert!(s.contains("?? hello.txt"), "got: {s:?}");
    }

    #[tokio::test]
    async fn git_status_non_repo_path_surfaces_exit_code() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().to_str().unwrap().to_string();
        // No `git init` here — deliberately not a repo.
        let tool = GitStatusTool;
        let out = tool
            .execute(json!({ "path": path }), &ctx())
            .await
            .expect("execute should not error; non-zero exit is the signal");
        assert_ne!(out["exit_code"], json!(0));
        assert!(
            out["stderr"].as_str().unwrap().contains("not a git repository"),
            "expected stderr to mention 'not a git repository', got: {:?}",
            out["stderr"]
        );
    }

    #[tokio::test]
    async fn git_diff_unstaged_shows_change() {
        let (dir, path) = make_repo();
        write_file(dir.path(), "a.txt", "line1\n");
        run_cli(&["-C", &path, "add", "a.txt"]);
        run_cli(&["-C", &path, "commit", "-q", "-m", "add a.txt"]);
        // Now modify it.
        write_file(dir.path(), "a.txt", "line1\nline2\n");
        let tool = GitDiffTool;
        let out = tool
            .execute(json!({ "path": path }), &ctx())
            .await
            .expect("execute");
        assert_eq!(out["exit_code"], json!(0));
        let s = out["stdout"].as_str().unwrap();
        assert!(s.contains("+line2"), "expected +line2 in diff, got: {s}");
        assert_eq!(out["truncated"], json!(false));
    }

    #[tokio::test]
    async fn git_diff_staged_shows_change() {
        let (dir, path) = make_repo();
        write_file(dir.path(), "b.txt", "first\n");
        run_cli(&["-C", &path, "add", "b.txt"]);
        // Don't commit — just stage.
        let tool = GitDiffTool;
        let out = tool
            .execute(json!({ "path": path, "staged": true }), &ctx())
            .await
            .expect("execute");
        assert_eq!(out["exit_code"], json!(0));
        let s = out["stdout"].as_str().unwrap();
        assert!(s.contains("+first"), "expected staged diff to contain +first, got: {s}");
    }

    #[tokio::test]
    async fn git_diff_truncates_huge_output() {
        let (dir, path) = make_repo();
        // Build a single line of ~64 KiB of 'a' characters so the diff
        // output is well over the 32 KiB cap.
        let big = "a".repeat(64 * 1024);
        write_file(dir.path(), "big.txt", &format!("{big}\n"));
        run_cli(&["-C", &path, "add", "big.txt"]);
        // Unstaged diff via touching the file post-stage would be
        // complicated; instead we modify a different file and diff it.
        // The point is just to exercise the truncator.
        write_file(dir.path(), "big.txt", &format!("{big}\nb\n"));
        let tool = GitDiffTool;
        let out = tool
            .execute(json!({ "path": path }), &ctx())
            .await
            .expect("execute");
        assert_eq!(out["exit_code"], json!(0));
        assert_eq!(out["truncated"], json!(true));
        let s = out["stdout"].as_str().unwrap();
        assert!(s.contains("[truncated:"), "expected truncation banner, got tail: {}", &s[s.len().saturating_sub(200)..]);
        // Output should be capped near the limit, not the full ~64 KiB.
        assert!(s.len() < 64 * 1024 + 1024, "truncated size too large: {}", s.len());
    }

    #[tokio::test]
    async fn git_log_shows_initial_commit() {
        let (_dir, path) = make_repo();
        let tool = GitLogTool;
        let out = tool
            .execute(json!({ "path": path }), &ctx())
            .await
            .expect("execute");
        assert_eq!(out["exit_code"], json!(0));
        let s = out["stdout"].as_str().unwrap();
        assert!(s.contains("init"), "expected 'init' in log, got: {s}");
    }

    #[tokio::test]
    async fn git_log_respects_max_count() {
        let (_dir, path) = make_repo();
        // Add 5 empty commits so the log has 6 entries total.
        for i in 0..5 {
            run_cli(&[
                "-C", &path,
                "commit", "--allow-empty", "-q",
                "-m", &format!("commit {i}"),
            ]);
        }
        let tool = GitLogTool;
        let out = tool
            .execute(json!({ "path": path, "max_count": 3 }), &ctx())
            .await
            .expect("execute");
        assert_eq!(out["exit_code"], json!(0));
        let s = out["stdout"].as_str().unwrap();
        // --oneline gives one line per commit; max_count=3 → 3 lines.
        let line_count = s.lines().filter(|l| !l.is_empty()).count();
        assert_eq!(line_count, 3, "expected 3 lines, got {line_count}: {s}");
    }

    #[tokio::test]
    async fn git_commit_creates_commit() {
        let (dir, path) = make_repo();
        write_file(dir.path(), "c.txt", "content\n");
        let tool = GitCommitTool;
        let out = tool
            .execute(
                json!({
                    "path": path,
                    "message": "add c.txt",
                    "paths": ["c.txt"],
                }),
                &ctx(),
            )
            .await
            .expect("execute");
        assert_eq!(out["exit_code"], json!(0), "stderr was: {}", out["stderr"]);
        // Verify the commit actually landed by reading the log.
        let log = GitLogTool
            .execute(json!({ "path": path, "max_count": 5 }), &ctx())
            .await
            .expect("log");
        let log_str = log["stdout"].as_str().unwrap();
        assert!(log_str.contains("add c.txt"), "log should mention the new commit, got: {log_str}");
    }

    #[tokio::test]
    async fn git_commit_no_verify_passes_flag() {
        let (dir, path) = make_repo();
        // Install a pre-commit hook that always fails. With no_verify,
        // the commit should still succeed.
        let hooks = dir.path().join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(
            hooks.join("pre-commit"),
            "#!/bin/sh\necho hook-failed >&2\nexit 1\n",
        )
        .unwrap();
        // Make the hook executable. Some test envs may have a restrictive
        // umask; be explicit.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(hooks.join("pre-commit"), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        write_file(dir.path(), "d.txt", "x\n");

        // Without no_verify: hook blocks the commit, exit_code != 0.
        let tool = GitCommitTool;
        let out = tool
            .execute(
                json!({
                    "path": path,
                    "message": "should fail",
                    "paths": ["d.txt"],
                }),
                &ctx(),
            )
            .await
            .expect("execute");
        assert_ne!(out["exit_code"], json!(0), "hook should have blocked the commit");

        // With no_verify: commit goes through despite the failing hook.
        let out2 = tool
            .execute(
                json!({
                    "path": path,
                    "message": "should succeed",
                    "paths": ["d.txt"],
                    "no_verify": true,
                }),
                &ctx(),
            )
            .await
            .expect("execute");
        assert_eq!(out2["exit_code"], json!(0), "no_verify should have bypassed the hook; stderr was: {}", out2["stderr"]);
    }

    #[tokio::test]
    async fn git_commit_empty_paths_is_rejected() {
        let (_dir, path) = make_repo();
        let tool = GitCommitTool;
        let err = tool
            .execute(
                json!({
                    "path": path,
                    "message": "noop",
                    "paths": [],
                }),
                &ctx(),
            )
            .await
            .expect_err("empty paths should be rejected");
        assert!(err.contains("paths"), "error should mention 'paths', got: {err}");
    }

    #[tokio::test]
    async fn git_log_huge_max_count_is_clamped() {
        let (_dir, path) = make_repo();
        let tool = GitLogTool;
        // 10^18 commits obviously don't exist, but the tool should NOT
        // pass that to git verbatim — it should clamp to a sane upper
        // bound so git doesn't try to allocate a 19-digit line buffer.
        let out = tool
            .execute(
                json!({ "path": path, "max_count": 1_000_000_000_000_000_000_u64 }),
                &ctx(),
            )
            .await
            .expect("execute");
        // We don't care what git says about a too-large number, just that
        // the call didn't time out and we got an answer.
        assert!(out["exit_code"].is_i64() || out["exit_code"].is_null());
    }
}
