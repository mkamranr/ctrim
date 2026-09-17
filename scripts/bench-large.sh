#!/usr/bin/env bash
# Generates a large synthetic log and measures ctrim's wall time and peak RSS.
#
#   scripts/bench-large.sh [size_mb]
#
# The design target is 100MB in under 200ms with under 20MB of memory.
set -euo pipefail

SIZE_MB="${1:-100}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT/tests/fixtures/generated"
LOG="$OUT_DIR/large_${SIZE_MB}mb.log"

mkdir -p "$OUT_DIR"
if [ ! -f "$LOG" ] || [ "$(wc -c <"$LOG")" -lt $((SIZE_MB * 1000000)) ]; then
  echo "generating ${SIZE_MB}MB fixture at $LOG"
  python3 - "$LOG" "$SIZE_MB" <<'PY'
import sys
path, size_mb = sys.argv[1], int(sys.argv[2])
target = size_mb * 1024 * 1024
block = []
for i in range(200):
    block.append(f'api_1 | 2024-05-01T10:00:{i%60:02d}.123Z level=info msg="handling request {i}" duration_ms={i%97}')
    if i % 5 == 0:
        block.append('api_1 | \x1b[33mWARN\x1b[0m retrying upstream connection, backing off 250ms')
    if i % 50 == 0:
        block.append('Traceback (most recent call last):')
        block.append('  File "app/main.py", line 42, in handler')
        block.append('    return service.run(payload)')
        for f in range(12):
            block.append(f'  File "/srv/.venv/lib/python3.11/site-packages/pkg{f}/mod.py", line {f}, in call')
            block.append('    return next_call()')
        block.append('ValueError: bad payload')
text = "\n".join(block) + "\n"
with open(path, "w") as fh:
    written = 0
    while written < target:
        fh.write(text)
        written += len(text)
PY
fi

cargo build --release --quiet
BIN="$ROOT/target/release/ctrim"
BYTES=$(wc -c <"$LOG")
echo "input: $((BYTES / 1024 / 1024))MB"

case "$(uname -s)" in
  Darwin) TIME_CMD=(/usr/bin/time -l) ;;
  *)      TIME_CMD=(/usr/bin/time -v) ;;
esac

"${TIME_CMD[@]}" "$BIN" --quiet --format raw "$LOG" >/dev/null
