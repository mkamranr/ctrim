# What gets removed, and why it is safe

`ctrim` has one hard rule:

> **Every line that survives is byte-identical to the input.** Nothing is
> summarised, paraphrased, reordered or rewritten. A line is either kept, or
> dropped and accounted for by an explicit `[...]` marker.

That rule is what makes it safe to put between your tools and a model. A
summariser can invent; a filter cannot.

---

## ANSI and carriage returns

Colour codes, cursor movement, OSC hyperlinks and DCS sequences are stripped.
Leading whitespace is preserved, because indentation carries meaning in traces
and diffs.

Progress bars are a special case. A bar writes many frames separated by `\r`,
and a terminal shows only the last one, so that is what survives:

```
10%\r50%\r100% done     ->     100% done
```

Backspaces (`\x08`) are applied the way a terminal would.

**Why it pays:** an escape sequence such as `ESC [ 1 m` costs roughly 1.5 tokens
beyond the punctuation it contains. A coloured `cargo check` run is about 40%
escape codes by token count.

`--preserve-ansi` turns all of this off.

---

## Repeated lines

Two mechanisms, both bounded in memory.

**Adjacent runs.** Consecutive lines with the same key collapse:

```
[Repeated 120 times: "connection to redis refused, retrying"]
```

**The window.** The last `--dedupe-window` (default 64) emitted lines are
remembered, so repeats interleaved with other output are still caught. These are
reported a few lines after the last occurrence, near the lines they describe:

```
[+100 more occurrences of: "connection to redis refused, retrying"]
```

### What counts as "the same line"

The comparison key is built in one pass over the bytes:

1. Leading and trailing whitespace trimmed, internal runs collapsed to one
   space. Indentation differences do not prevent folding.
2. With fuzzy folding on (the default), each run of digits becomes `#`, and any
   alphanumeric run of 8+ characters containing a digit collapses entirely.

So these three fold together:

```
2024-05-01T10:00:04Z WARN retry attempt=41 request=9f8e7d6c5b4a3210
2024-05-01T10:00:05Z WARN retry attempt=42 request=1a2b3c4d5e6f7890
2024-05-01T10:00:06Z WARN retry attempt=43 request=fedcba0987654321
```

The note shows the **first line verbatim**, so you keep one real example. The
other numbers are gone — that is the trade, and `--exact-dupes` opts out of it.

### Guards against over-folding

- Lines shorter than 24 characters only fold when **adjacent**, never through
  the window. `}` and `---` repeat legitimately in source and diffs.
- Blank runs collapse to a single blank line with no note, because a note would
  cost more tokens than the lines it replaces.
- The diff parser runs instead of the deduplicator on diff input, so repeated
  source lines are never folded.

---

## Stack traces

Four dialects are recognised:

| Dialect | Frame shape |
| :--- | :--- |
| Python | `File "app/main.py", line 42, in handler` after `Traceback (most recent call last):` |
| pytest long form | `tests/test_api.py:12: in test_get` |
| JavaScript / Node | `    at handler (/srv/app/index.js:10:15)` |
| Rust backtrace | `   7: core::panicking::panic_fmt` plus its `at` line |

Each frame is classified **user** or **vendor** by path:

```
/node_modules/   /site-packages/   /dist-packages/   /.venv/   /venv/lib/
/.pyenv/   /usr/lib/python   /lib/python3   <frozen    node:internal/
/rustc/   /.cargo/registry/   /.rustup/toolchains/   /go/pkg/mod/   /vendor/
(<anonymous>)   (native)
```

Then:

- **Every user frame is kept.** Always.
- **The exception line is kept.** Always — it is also lifted into
  `<error_summary>` for the XML formatter.
- Each contiguous vendor run longer than `--keep-frames` (default 3) is replaced
  by `[... N vendor frames omitted ...]`, keeping the frames **closest to the
  failure**, which are the ones that show how your code entered the library.
- The outermost frame is pinned for Python, pytest and JavaScript, because it is
  the entry point. It is not pinned for Rust, where frame 0 is
  `rust_begin_unwind` — pure panic machinery.

A trace still open when the input ends is flushed, not lost.

**Why it pays:** a Flask or Jest traceback is routinely 15 frames of framework
for 2 frames of your code. The model needs your 2.

---

## Diffs

**Context trimming.** Context lines beyond `--context-lines` (default 1) either
side of a change run are dropped. The hunk is split at the gaps and each region
gets its own `@@` header with **recomputed line counts**, so the result is still
a valid patch that `git apply` accepts.

```diff
@@ -11,3 +11,3 @@
 two
-three
+THREE
 four
```

**Generated files.** These collapse to one line each:

```
Cargo.lock  package-lock.json  yarn.lock  pnpm-lock.yaml  composer.lock
Gemfile.lock  poetry.lock  go.sum  flake.lock  bun.lockb
*.lock  *.min.js  *.min.css  *.map  *.pb.go  *_pb2.py
```

```
[skipped generated file Cargo.lock: +412/-87 lines]
```

The counts are real — they are tallied from the hunks before they are dropped.

**Binary files** become `[binary file logo.png changed]`.

**`index 89abc12..def3456` lines** are removed. A blob hash tells a model
nothing.

**Context-only hunks** (a hunk with no `+` or `-` lines, which some tools emit)
are dropped entirely.

---

## Detection

The first 200 lines are scored against weighted signatures — `diff --git` scores
6 for git diffs, `error[E0308]:` scores 6 for cargo, `=== FAILURES ===` scores 6
for pytest, and so on. The highest score above 4 wins; anything less confident
falls back to `generic-log`.

Detection runs on **ANSI-stripped** text, because colour codes hide the very
line starts the signatures look for.

`--preset` skips detection entirely.

---

## What `ctrim` deliberately does not do

- **Summarise.** No model, no paraphrase, no "the test failed because…".
- **Reorder.** Output order matches input order.
- **Truncate blindly.** There is no "keep the first N lines" mode; every removal
  is a rule you can read on this page.
- **Phone home.** No network access of any kind.
