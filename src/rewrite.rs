use std::{
    ffi::OsStr,
    io::Write,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};

use crate::{
    cpu::{self, Hit},
    gpu, object,
    sha1mid::PrefixMask,
};

#[derive(Clone, Copy)]
pub struct MineOptions {
    pub threads: usize,
    pub gpu: bool,
    pub force: bool,
}

pub fn mine(prefix: &str, options: MineOptions) -> Result<String> {
    let mask = PrefixMask::parse(prefix)?;
    check_mining_options(options)?;
    if options.gpu {
        gpu::check_available()?;
    }
    check_sha1_repo()?;
    let old_head = rev_parse("HEAD")?;
    if !options.force {
        refuse_remote_commits(std::slice::from_ref(&old_head))?;
    }
    let raw = cat_commit(&old_head)?;
    let (raw, signed) = object::strip_gpgsig(&raw);
    if signed {
        eprintln!("warning: stripped gpgsig from {old_head}");
    }

    let backup = save_backup(&old_head)?;
    let (content, hit) = mine_bytes(&raw, &mask, options)?;
    let new_hash = write_commit(&content)?;
    verify_hash(&new_hash, &hit)?;
    update_head(&new_hash, &old_head)?;
    println!("{old_head} -> {new_hash}");
    println!("backup: {backup}");
    Ok(new_hash)
}

pub fn spray(prefixes: &[String], options: MineOptions, dry_run: bool) -> Result<Vec<String>> {
    if prefixes.is_empty() {
        bail!("spray needs at least one prefix");
    }
    let masks = prefixes
        .iter()
        .map(|prefix| PrefixMask::parse(prefix))
        .collect::<Result<Vec<_>>>()?;
    if !dry_run {
        check_mining_options(options)?;
        if options.gpu {
            gpu::check_available()?;
        }
    }
    check_sha1_repo()?;
    let old_head = rev_parse("HEAD")?;
    let commits = recent_commits(prefixes.len())?;
    if commits.len() != prefixes.len() {
        bail!(
            "asked for {} commits, but this branch only has {}",
            prefixes.len(),
            commits.len()
        );
    }

    let mut raw_commits = Vec::with_capacity(commits.len());
    let mut signed = Vec::new();
    for commit in &commits {
        let raw = cat_commit(commit)?;
        let parents = object::parent_hashes(&raw);
        if parents.len() > 1 {
            bail!("commit {commit} is a merge. spray only handles linear history for now");
        }
        let (clean, did_strip) = object::strip_gpgsig(&raw);
        if did_strip {
            signed.push(commit.clone());
        }
        raw_commits.push(clean);
    }

    if dry_run {
        if !signed.is_empty() {
            eprintln!("warning: would strip gpgsig from {}", signed.join(", "));
        }
        for (commit, prefix) in commits.iter().zip(prefixes) {
            println!("{} -> {}...", short(commit), prefix.to_ascii_lowercase());
        }
        return Ok(Vec::new());
    }

    if !options.force {
        refuse_remote_commits(&commits)?;
    }
    if !signed.is_empty() {
        eprintln!("warning: stripped gpgsig from {}", signed.join(", "));
    }

    let backup = save_backup(&old_head)?;
    let mut mined = Vec::with_capacity(commits.len());
    let mut new_parent: Option<String> = None;
    for (((old_hash, raw), mask), prefix) in commits
        .iter()
        .zip(raw_commits)
        .zip(masks.iter())
        .zip(prefixes)
    {
        let rewritten = match &new_parent {
            Some(parent) => object::replace_parent(&raw, parent),
            None => raw,
        };
        let (content, hit) = mine_bytes(&rewritten, mask, options)
            .with_context(|| format!("could not mine {prefix} for {old_hash}"))?;
        let hash = write_commit(&content)?;
        verify_hash(&hash, &hit)?;
        println!("{} -> {}", short(old_hash), hash);
        new_parent = Some(hash.clone());
        mined.push(hash);
    }

    let new_head = mined.last().context("spray had no commits")?;
    update_head(new_head, &old_head)?;
    println!("backup: {backup}");
    Ok(mined)
}

pub fn undo() -> Result<String> {
    check_sha1_repo()?;
    let output = git_output([
        "for-each-ref",
        "--sort=-refname",
        "--format=%(refname) %(objectname)",
        "refs/graffiti/backup/",
    ])?;
    let line = output
        .lines()
        .next()
        .context("no graffiti backup found. there is nothing to undo")?;
    let (reference, hash) = line
        .split_once(' ')
        .context("git returned a broken backup ref")?;
    let old_head = rev_parse("HEAD")?;
    update_head(hash, &old_head)?;
    git_ok(["update-ref", "-d", reference, hash])?;
    println!("restored {hash}");
    Ok(hash.to_owned())
}

fn mine_bytes(raw: &[u8], mask: &PrefixMask, options: MineOptions) -> Result<(Vec<u8>, Hit)> {
    let prepared = object::prepare(raw)?;
    let hit = if options.gpu {
        gpu::search(&prepared, mask)?
    } else {
        cpu::search(&prepared, mask, options.threads)?
    };
    Ok((prepared.content_for_nonce(hit.nonce), hit))
}

fn check_mining_options(options: MineOptions) -> Result<()> {
    if !options.gpu && options.threads == 0 {
        bail!("thread count is zero. use at least one thread");
    }
    Ok(())
}

fn verify_hash(written: &str, hit: &Hit) -> Result<()> {
    let expected = hex_digest(&hit.digest);
    if written != expected {
        bail!("git wrote {written}, but the miner got {expected}. refusing to move HEAD");
    }
    Ok(())
}

fn check_sha1_repo() -> Result<()> {
    let output = Command::new("git")
        .args(["config", "--get", "extensions.objectformat"])
        .output()
        .context("could not run git config")?;
    if output.status.success() {
        let format = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if format.eq_ignore_ascii_case("sha256") {
            bail!("this repository uses SHA-256. git-graffiti only handles SHA-1 repositories");
        }
        if !format.is_empty() && !format.eq_ignore_ascii_case("sha1") {
            bail!("this repository uses unknown object format {format:?}");
        }
    }
    rev_parse("--git-dir")?;
    Ok(())
}

fn recent_commits(count: usize) -> Result<Vec<String>> {
    let count_arg = format!("--max-count={count}");
    let output = git_output(["rev-list", "--reverse", &count_arg, "HEAD"])?;
    Ok(output.lines().map(ToOwned::to_owned).collect())
}

fn cat_commit(hash: &str) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(["cat-file", "commit", hash])
        .output()
        .with_context(|| format!("could not read commit {hash}"))?;
    if !output.status.success() {
        bail!(
            "git cat-file could not read commit {hash}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn write_commit(content: &[u8]) -> Result<String> {
    let mut child = Command::new("git")
        .args(["hash-object", "-t", "commit", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run git hash-object")?;
    child
        .stdin
        .take()
        .context("git hash-object had no stdin")?
        .write_all(content)
        .context("could not send commit to git hash-object")?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "git hash-object failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn save_backup(head: &str) -> Result<String> {
    let mut timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("the system clock is before 1970")?
        .as_secs();
    loop {
        let reference = format!("refs/graffiti/backup/{timestamp}");
        let status = Command::new("git")
            .args(["show-ref", "--verify", "--quiet", &reference])
            .status()?;
        if !status.success() {
            git_ok([
                "update-ref",
                &reference,
                head,
                "0000000000000000000000000000000000000000",
            ])?;
            return Ok(reference);
        }
        timestamp += 1;
    }
}

fn refuse_remote_commits(commits: &[String]) -> Result<()> {
    let refs = git_output(["for-each-ref", "--format=%(refname)", "refs/remotes/"])?;
    for commit in commits {
        for remote_ref in refs.lines() {
            let status = Command::new("git")
                .args(["merge-base", "--is-ancestor", commit, remote_ref])
                .status()?;
            if status.success() {
                bail!(
                    "commit {commit} is on {remote_ref}. pass --force if rewriting it is really what you want"
                );
            }
            if status.code() != Some(1) {
                bail!("git merge-base failed while checking {remote_ref}");
            }
        }
    }
    Ok(())
}

fn update_head(new_hash: &str, old_hash: &str) -> Result<()> {
    git_ok(["update-ref", "HEAD", new_hash, old_hash])
}

fn rev_parse(value: &str) -> Result<String> {
    git_output(["rev-parse", value])
}

fn git_output<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .output()
        .context("could not run git")?;
    if !output.status.success() {
        bail!(
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn git_ok<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .output()
        .context("could not run git")?;
    if !output.status.success() {
        bail!(
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn short(hash: &str) -> &str {
    hash.get(..7).unwrap_or(hash)
}

pub fn hex_digest(digest: &[u8; 20]) -> String {
    use std::fmt::Write as _;

    let mut hex = String::with_capacity(40);
    for byte in digest {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}
