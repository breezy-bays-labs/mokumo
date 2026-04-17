#!/usr/bin/env bash
# Regenerates tests/fixtures/pre-stage3.sqlite + session_continuity.env from
# commit bfae58e (Stage 1b merged). Must run on a host session — container
# workspaces cannot create git worktrees inside /workspace (see AGENTS.md).
set -euo pipefail

CAPTURE_COMMIT="bfae58e"
PORT="${PORT:-16565}"
REPO_ROOT="$(git rev-parse --show-toplevel)"
FIXTURE_DIR="$REPO_ROOT/tests/fixtures"
SCRATCH="$(mktemp -d)/stage3-capture"
WORKTREE="$SCRATCH/worktree"
DATA_DIR="$SCRATCH/profile"
TARGET_DIR="$SCRATCH/target"
LOG="$SCRATCH/server.log"
JAR="$SCRATCH/cookie.jar"

cleanup() {
  if [[ -n "${API_PID:-}" ]]; then kill -INT "$API_PID" 2>/dev/null || true; wait "$API_PID" 2>/dev/null || true; fi
  git -C "$REPO_ROOT" worktree remove --force "$WORKTREE" 2>/dev/null || true
  rm -rf "$SCRATCH"
}
trap cleanup EXIT

if [[ ! -d "$REPO_ROOT/apps/web/build" ]]; then
  echo "!! apps/web/build not found; run 'moon run web:build' before regenerating the fixture" >&2
  exit 1
fi

echo "== creating scratch worktree on $CAPTURE_COMMIT"
git -C "$REPO_ROOT" worktree add "$WORKTREE" "$CAPTURE_COMMIT"
# rust-embed needs apps/web/build present at compile time
cp -r "$REPO_ROOT/apps/web/build" "$WORKTREE/apps/web/build"

echo "== building mokumo-api from $CAPTURE_COMMIT (debug)"
(
  cd "$WORKTREE"
  CARGO_TARGET_DIR="$TARGET_DIR" cargo build --bin mokumo-api
)

echo "== launching server on port $PORT"
mkdir -p "$DATA_DIR"
"$TARGET_DIR/debug/mokumo-api" --data-dir "$DATA_DIR" --host 127.0.0.1 --port "$PORT" > "$LOG" 2>&1 &
API_PID=$!

echo "== waiting for health"
healthy=0
for _ in $(seq 30); do
  if curl -sf "http://127.0.0.1:${PORT}/api/health" >/dev/null; then healthy=1; break; fi
  sleep 1
done
if [[ "$healthy" -ne 1 ]]; then
  echo "!! server never became healthy on port $PORT; tail of $LOG:" >&2
  tail -50 "$LOG" >&2 || true
  exit 1
fi

TOKEN="$(grep -a -o 'token: [0-9a-f-]\{36\}' "$LOG" | head -1 | awk '{print $2}')"
if [[ -z "$TOKEN" ]]; then
  echo "!! failed to scrape setup token from $LOG" >&2
  exit 1
fi
EMAIL="capture+$(openssl rand -hex 4)@pre-stage3.local"
PASSWORD="stage3-capture-$(openssl rand -hex 8)"

echo "== POST /api/setup"
curl -sf -c "$JAR" -b "$JAR" -H 'Content-Type: application/json' \
  -d "$(jq -n --arg e "$EMAIL" --arg p "$PASSWORD" --arg t "$TOKEN" \
       '{admin_email:$e, admin_name:"Capture", admin_password:$p, shop_name:"Capture Shop", setup_token:$t}')" \
  "http://127.0.0.1:${PORT}/api/setup" > /dev/null

echo "== POST /api/customers"
curl -sf -c "$JAR" -b "$JAR" -H 'Content-Type: application/json' \
  -d '{"display_name":"Pre-Stage-3 Capture Customer"}' \
  "http://127.0.0.1:${PORT}/api/customers" > /dev/null

SESSION_COOKIE="$(awk '/HttpOnly_127.0.0.1.*\tid\t/ { print $NF }' "$JAR")"
if [[ -z "$SESSION_COOKIE" ]]; then
  echo "!! failed to scrape session cookie from $JAR" >&2
  cat "$JAR" >&2 || true
  exit 1
fi

echo "== shutting down server"
kill -INT "$API_PID"
wait "$API_PID" 2>/dev/null || true
API_PID=""

echo "== checkpointing WAL"
if command -v sqlite3 >/dev/null 2>&1; then
  sqlite3 "$DATA_DIR/production/mokumo.db" "PRAGMA wal_checkpoint(TRUNCATE);"
else
  python3 - <<PY
import sqlite3
conn = sqlite3.connect("$DATA_DIR/production/mokumo.db")
conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
conn.close()
PY
fi

echo "== copying fixture to $FIXTURE_DIR"
mkdir -p "$FIXTURE_DIR"
cp "$DATA_DIR/production/mokumo.db" "$FIXTURE_DIR/pre-stage3.sqlite"
cat > "$FIXTURE_DIR/session_continuity.env" <<EOF
host=127.0.0.1:${PORT}
pre_stage3_session=${SESSION_COOKIE}
alice_email=${EMAIL}
alice_password=${PASSWORD}
expected_session_name=id
EOF

cat > "$FIXTURE_DIR/README.md" <<EOF
# Pre-Stage-3 capture fixtures

Regenerate via: \`bash scripts/capture-pre-stage3-fixture.sh\`
Captured from: ${CAPTURE_COMMIT} on $(date -u +%Y-%m-%d)

Contents of \`pre-stage3.sqlite\`:
- 1 admin user (${EMAIL})
- 1 customer row (display_name = "Pre-Stage-3 Capture Customer")
- 2 activity_log rows (login_success on setup + customer_created)
- 8 applied migrations in \`seaql_migrations\` (pre-S2.1 — kikan_migrations bootstrap runs on replay)

Used by:
- \`crates/kikan/tests/migration_replay_snapshot.rs\` — proves the runner backfills seaql into kikan_migrations and re-applies none of the pre-Stage-3 migrations.
- Future \`tests/api/session_continuity.hurl\` — proves session cookies minted by pre-Stage-3 code remain valid after the platform lift.
EOF

echo "== fixture regenerated"
