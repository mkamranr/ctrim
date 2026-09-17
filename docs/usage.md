# Usage

```console
$ ctrim [OPTIONS] [FILE]
$ <command> | ctrim [OPTIONS]
```

`ctrim` reads STDIN, or a file if you name one. It writes the compressed result
to STDOUT and a one-line summary to STDERR, so the summary never lands in the
text you paste.

---

## Flags

### `--format <markdown|xml|raw>`, `-f`

How the result is wrapped. Default `markdown`.

| Value | Output |
| :--- | :--- |
| `markdown` | Fenced code block, language chosen from the detected format, plus an `**Errors**` list when a trace was folded |
| `xml` | `<context type="pytest">…</context>` and `<error_summary>…</error_summary>` |
| `raw` | The processed lines, nothing added. Use this when piping into another tool |

```console
$ git diff | ctrim --format xml
<context type="git-diff">
diff --git a/src/handler.rs b/src/handler.rs
@@ -3,3 +3,3 @@
     // routine line 2 of the request handler
-    // routine line 3 of the request handler
+    let user = authenticate(&req)?;
</context>
```

### `--preset <auto|pytest|cargo|diff|docker|json|generic>`, `-p`

Which parser to run. `auto` scores the first 200 lines and picks; every other
value pins the choice. Pin it when detection guesses wrong, or when you are
piping a fragment too short to be recognisable.

```console
$ tail -n 40 build.log | ctrim --preset cargo
```

### `--clip`, `-c`

Copy the result to the system clipboard. If no clipboard exists — headless
Linux, a container, CI — `ctrim` says so on STDERR and writes STDOUT instead,
rather than failing.

### `--out <FILE>`, `-o`

Write to a file. STDOUT stays empty, which is convenient in scripts.

### `--keep-frames <N>`, `-k`

Vendor stack frames kept per folded run. Default `3`. `0` removes every vendor
frame, which is usually what you want when the bug is clearly in your own code:

```console
$ pytest 2>&1 | ctrim --keep-frames 0
```

### `--context-lines <N>`

Unified-diff context kept either side of each change. Default `1`. Git emits 3,
so the default already removes two thirds of the context a normal diff carries
while keeping the patch valid. Raise it if you are asking a model to reason
about surrounding code:

```console
$ git diff | ctrim --context-lines 3    # what git gave us, unchanged
```

### `--dedupe-window <N>`

How many recent lines the repeated-line detector remembers, for repeats that are
interleaved with other output rather than adjacent. Default `64`. Raise it for
logs where two subsystems interleave heavily; lower it to reduce the chance of
folding lines that only look alike.

### `--dedupe-only`

Run just the repeated-line compressor. Stack traces, diffs and everything else
pass through untouched. Use it when you want the log shortened but need every
frame:

```console
$ docker logs app 2>&1 | ctrim --dedupe-only
```

### `--exact-dupes`

Fold only byte-identical lines. By default `ctrim` also folds lines that differ
only in their numbers and ids (`attempt=41` with `attempt=42`), which is what
collapses retry storms — but it means the individual numbers are gone. Turn it
off when those numbers matter:

```console
$ ctrim --exact-dupes deploy.log
```

### `--preserve-ansi`

Keep colour escape sequences. They cost real tokens (an escape sequence runs
about 1.5 tokens beyond its punctuation), so this is off by default.

### `--quiet`, `-q`

Suppress the STDERR summary.

---

## Recipes

### Straight into the clipboard

```bash
pytest 2>&1 | ctrim --clip
cargo check 2>&1 | ctrim --clip
git diff | ctrim --clip
```

Add aliases to your shell profile:

```bash
alias t='ctrim --clip'
alias tdiff='git diff | ctrim --format xml --clip'
alias tlog='ctrim --dedupe-only'
```

### Feeding a coding agent from a file

```bash
pytest 2>&1 | ctrim --format xml --out /tmp/failure.xml
```

Then point the agent at `/tmp/failure.xml`. XML survives copy-paste better than
fenced Markdown, because the model cannot confuse your log's backticks with the
fence.

### Inside CI

Attach a compressed failure log to the job summary instead of a 50k-line raw
log:

```yaml
- name: Test
  run: pytest 2>&1 | tee raw.log
- name: Compress the failure log
  if: failure()
  run: |
    ctrim raw.log --format markdown >> "$GITHUB_STEP_SUMMARY"
```

### Staged diffs, excluding the noise git cannot exclude

```bash
git diff --cached | ctrim --format xml --clip
```

Lockfiles, `*.min.js`, `*.map` and generated protobuf files collapse to one
summary line each — no `:(exclude)` pathspecs needed.

### Long-running containers

```bash
docker logs --since 1h api 2>&1 | ctrim --preset docker
kubectl logs deploy/api --tail=5000 | ctrim
```

### Checking what a reduction cost you

```bash
ctrim --format raw build.log > trimmed.log
diff <(ctrim --format raw --preserve-ansi --dedupe-only build.log) trimmed.log
```

Every line `ctrim` removed is either a duplicate it counted or a frame it
marked. There is no third category.

---

## Exit codes

| Code | Meaning |
| :--- | :--- |
| `0` | Success, including empty input |
| `2` | Could not read the input file, could not write the output |

Running `ctrim` with no file and no pipe prints a usage hint and exits `2`,
rather than hanging on an interactive terminal.

---

## Tuning for a project

There is no config file yet (it is on the roadmap in
[CHANGELOG.md](../CHANGELOG.md)). Until then, wrap your preferences in a shell
function:

```bash
ctrim_py() {
  ctrim --preset pytest --keep-frames 1 --dedupe-window 128 "$@"
}
```
