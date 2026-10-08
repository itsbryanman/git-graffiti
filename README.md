# git-graffiti

Spray paint your git history.

```
$ git log --oneline
0ddba11 wire up the gpu path
c0ffee0 stop leaking file handles
bada55e rewrite the nonce encoder
deadbee fix off by one in the walker
5ca1ab1 initial commit
```

None of those hashes are an accident. git-graffiti grinds SHA-1 until each commit hash starts with whatever hex you tell it to, then rewrites your history so `git log` reads like a message. Your code doesn't change. Your commit messages look the same. The hashes are just... different now.

It's useless. I love it.

## How it works

A git commit hash is the SHA-1 of the raw commit object:

```
commit <length>\0
tree 9c1e...
parent 4ab0...
author Bryan <...> 1728400000 -0400
committer Bryan <...> 1728400000 -0400

fix off by one in the walker
```

Change one byte and you get a completely different hash. So git-graffiti appends a run of spaces and tabs to the end of the commit message. Space is 0, tab is 1. That's the nonce. It's trailing whitespace, so `git log` and GitHub don't show it, and the length is fixed so the `commit <length>` header never moves.

Then it counts. Try nonce 0, hash, check the prefix. Try nonce 1. Keep going until the first N hex chars match.

Two tricks make that fast:

- **Midstate caching.** Everything before the nonce is identical on every attempt, so the SHA-1 state for those blocks is computed once. The message is padded so the nonce always lands in the final 64-byte block, which means each attempt is a single block compression instead of rehashing the whole commit.
- **Raw byte compare.** Prefixes are checked as masked bytes, not hex strings. Odd-length prefixes use a nibble mask.

Rewriting history is the annoying part. Every commit's hash depends on its parent's hash, so you can't mine them in parallel. git-graffiti walks oldest to newest, rewrites each commit's `parent` line to point at the freshly mined parent, then mines that commit. Parallelism happens inside each commit's search, across every core (or the GPU).

## How hard is it

Each hex char is 4 bits, so every extra char is 16x the work. Expected attempts for a prefix of length n is 16^n.

| prefix | bits | expected attempts | CPU (8 threads) | GPU |
|---|---|---|---|---|
| 5 | 20 | ~1M | <!-- bench --> | <!-- bench --> |
| 6 | 24 | ~16.7M | <!-- bench --> | <!-- bench --> |
| 7 | 28 | ~268M | <!-- bench --> | <!-- bench --> |
| 8 | 32 | ~4.3B | <!-- bench --> | <!-- bench --> |

Numbers come from `cargo bench` on my machine. Yours will be different. 7 chars is the sweet spot because that's what `--oneline` and GitHub show.

## Install

```
cargo install git-graffiti
```

GPU support needs OpenCL:

```
cargo install git-graffiti --features opencl
```

Because the binary is named `git-graffiti`, git picks it up as a subcommand. `git graffiti` just works.

## Usage

Mine a prefix onto HEAD:

```
git graffiti mine c0ffee0
```

Spray a message across the last few commits, oldest first:

```
git graffiti spray 5ca1ab1 deadbee bada55e c0ffee0 0ddba11
```

Not sure what you can spell? Hex only gives you `0-9` and `a-f`, so it's leetspeak or nothing. `words` helps:

```
$ git graffiti words "bad coffee"
bad  -> bad
coffee -> c0ffee
```

It'll tell you when a word can't be done. There's no `r`. I'm sorry.

See what your log would look like without touching anything:

```
git graffiti spray --dry-run 5ca1ab1 deadbee bada55e
```

Undo:

```
git graffiti undo
```

## Flags

```
--gpu            use OpenCL instead of CPU threads
--threads <n>    CPU threads (default: all of them)
--dry-run        mine nothing, print the plan
--force          allow rewriting commits that are already on a remote
```

## Things that will bite you

**This rewrites history.** Every commit from the first sprayed one to HEAD gets a new hash. If you pushed already, you'll need `git push --force`. Do this on your own repos, not on a branch your coworkers are building on.

**Signed commits lose their signatures.** The signature covers the old bytes. git-graffiti strips `gpgsig` headers and warns you. Re-sign afterward if you care.

**Merge commits aren't supported yet.** It'll refuse and tell you which commit is the problem. Linear history only for now.

**SHA-256 repos aren't supported yet.** `git init --object-format=sha256` repos get a clear error.

**Long abbreviations.** Git grows the `--oneline` hash length as repos get bigger. If your 7-char art shows up as 9 chars, run `git config core.abbrev 7`.

**It always makes a backup.** Before anything gets rewritten, the old HEAD is saved to `refs/graffiti/backup/<unix-time>`. That's what `undo` restores. `git fsck` passes on everything git-graffiti writes, and the test suite checks it.

## Prior art

[lucky-commit](https://github.com/not-an-aardvark/lucky-commit) by not-an-aardvark did single-commit prefix mining first and does it well. Go look at it. git-graffiti is about the whole column: planning a message across a history and rewriting it in one shot.

## License

MIT
