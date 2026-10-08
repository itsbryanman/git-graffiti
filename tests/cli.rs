use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use sha1::{Digest, Sha1};
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .expect("run git")
}

fn git_ok(repo: &Path, args: &[&str]) -> String {
    let output = git(repo, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output was utf-8")
        .trim()
        .to_owned()
}

fn graffiti(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_git-graffiti"))
        .current_dir(repo)
        .args(args)
        .output()
        .expect("run git-graffiti")
}

fn graffiti_ok(repo: &Path, args: &[&str]) -> String {
    let output = graffiti(repo, args);
    assert!(
        output.status.success(),
        "git-graffiti {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("graffiti output was utf-8")
}

fn repo_with_commits(count: usize) -> TempDir {
    let temp = TempDir::new().expect("make temp repo");
    git_ok(temp.path(), &["init", "-q"]);
    git_ok(temp.path(), &["config", "user.name", "Test Person"]);
    git_ok(temp.path(), &["config", "user.email", "test@example.com"]);
    for index in 0..count {
        fs::write(temp.path().join("file.txt"), format!("revision {index}\n"))
            .expect("write fixture");
        git_ok(temp.path(), &["add", "file.txt"]);
        git_ok(
            temp.path(),
            &["commit", "-q", "-m", &format!("commit {index}")],
        );
    }
    temp
}

#[test]
fn mine_writes_the_hash_git_reports() {
    let repo = repo_with_commits(1);
    graffiti_ok(repo.path(), &["mine", "0bad"]);

    let head = git_ok(repo.path(), &["rev-parse", "HEAD"]);
    assert!(head.starts_with("0bad"), "mined hash was {head}");

    let content = git(repo.path(), &["cat-file", "commit", "HEAD"]).stdout;
    let mut object = format!("commit {}\0", content.len()).into_bytes();
    object.extend_from_slice(&content);
    let independently_hashed = format!("{:x}", Sha1::digest(object));
    assert_eq!(head, independently_hashed);
}

#[test]
fn spray_keeps_trees_valid_and_undo_restores_head() {
    let repo = repo_with_commits(5);
    let old_head = git_ok(repo.path(), &["rev-parse", "HEAD"]);
    let old_hashes = git_ok(repo.path(), &["rev-list", "--reverse", "HEAD"]);
    let old_hashes: Vec<_> = old_hashes.lines().map(ToOwned::to_owned).collect();
    let old_trees = git_ok(repo.path(), &["log", "--reverse", "--format=%T"]);

    graffiti_ok(repo.path(), &["spray", "cafe", "babe", "f00d"]);
    let hashes = git_ok(repo.path(), &["rev-list", "--reverse", "HEAD"]);
    let hashes: Vec<_> = hashes.lines().collect();
    assert_eq!(hashes.len(), 5);
    assert_eq!(hashes[..2], old_hashes[..2]);
    for (hash, prefix) in hashes[2..].iter().zip(["cafe", "babe", "f00d"]) {
        assert!(
            hash.starts_with(prefix),
            "{hash} did not start with {prefix}"
        );
    }

    let new_trees = git_ok(repo.path(), &["log", "--reverse", "--format=%T"]);
    assert_eq!(old_trees, new_trees);
    git_ok(repo.path(), &["fsck", "--strict"]);

    graffiti_ok(repo.path(), &["undo"]);
    assert_eq!(git_ok(repo.path(), &["rev-parse", "HEAD"]), old_head);
}

#[test]
fn spray_refuses_merge_commits() {
    let repo = repo_with_commits(1);
    git_ok(repo.path(), &["checkout", "-q", "-b", "side"]);
    fs::write(repo.path().join("side.txt"), "side\n").expect("write side file");
    git_ok(repo.path(), &["add", "side.txt"]);
    git_ok(repo.path(), &["commit", "-q", "-m", "side"]);
    git_ok(repo.path(), &["checkout", "-q", "master"]);
    fs::write(repo.path().join("main.txt"), "main\n").expect("write main file");
    git_ok(repo.path(), &["add", "main.txt"]);
    git_ok(repo.path(), &["commit", "-q", "-m", "main"]);
    git_ok(
        repo.path(),
        &["merge", "-q", "--no-ff", "side", "-m", "merge"],
    );

    let head = git_ok(repo.path(), &["rev-parse", "HEAD"]);
    let output = graffiti(repo.path(), &["spray", "dead"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("commit {head} is a merge")),
        "{stderr}"
    );
}

#[test]
fn sha256_repo_is_refused() {
    let temp = TempDir::new().expect("make temp repo");
    let init = git(temp.path(), &["init", "-q", "--object-format=sha256"]);
    assert!(
        init.status.success(),
        "this git does not support SHA-256 test repositories: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    git_ok(temp.path(), &["config", "user.name", "Test Person"]);
    git_ok(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("file.txt"), "sha256\n").expect("write fixture");
    git_ok(temp.path(), &["add", "file.txt"]);
    git_ok(temp.path(), &["commit", "-q", "-m", "sha256"]);

    let output = graffiti(temp.path(), &["mine", "dead"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("repository uses SHA-256"));
}

#[test]
fn dry_run_does_not_move_head_or_make_a_backup() {
    let repo = repo_with_commits(2);
    let old_head = git_ok(repo.path(), &["rev-parse", "HEAD"]);
    let output = graffiti_ok(repo.path(), &["spray", "--dry-run", "dead", "beef"]);
    assert!(output.contains("dead..."));
    assert!(output.contains("beef..."));
    assert_eq!(git_ok(repo.path(), &["rev-parse", "HEAD"]), old_head);
    let backups = git_ok(
        repo.path(),
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/graffiti/backup/",
        ],
    );
    assert!(backups.is_empty());
}

#[test]
fn bad_prefix_is_rejected_before_git_is_opened() {
    let temp = TempDir::new().expect("make temp directory");
    let output = graffiti(temp.path(), &["mine", "nope"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not hex"));
}
