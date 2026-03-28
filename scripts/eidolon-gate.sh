#!/bin/bash
# eidolon-gate.sh -- standalone gate hook for manual Claude Code sessions
# Place in .claude/settings.json as a PreToolUse hook command
# Usage: bash /path/to/eidolon-gate.sh "$TOOL_NAME" "$TOOL_INPUT"

EIDOLON_URL="${EIDOLON_URL:-http://127.0.0.1:7700}"
GATE_URL="${EIDOLON_URL}/gate/check"

INPUT=$(cat)

# Fail open if daemon not reachable
RESP=$(echo "$INPUT" | curl -sf --max-time 3 -X POST "$GATE_URL" \
  -H "Content-Type: application/json" \
  -d @- 2>/dev/null)

if [ $? -ne 0 ]; then
  # Daemon unreachable -- allow (fail open)
  exit 0
fi

ACTION=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('action','allow'))" 2>/dev/null)
if [ $? -ne 0 ]; then
  exit 0
fi

if [ "$ACTION" = "block" ]; then
  MSG=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('message','blocked by gate'))" 2>/dev/null)
  echo "$MSG" >&2
  exit 2
fi

# For enrich: print context to stderr (informational, does not block)
if [ "$ACTION" = "enrich" ]; then
  CTX=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('message',''))" 2>/dev/null)
  if [ -n "$CTX" ]; then
    echo "[eidolon-gate] $CTX" >&2
  fi
fi

exit 0
