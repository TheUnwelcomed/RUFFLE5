#!/usr/bin/env bash
# PS5_RUST_SEND_V1 — send the payload to elfldr on the console and show what it prints.
# usage: ./send.sh <ps5-ip> [port]      (elfldr listens on 9021)
set -euo pipefail
HOST="${1:?usage: ./send.sh <ps5-ip> [port]}"; PORT="${2:-9021}"
ELF="target/x86_64-sie-ps5/release/ps5-rust-hello"
[[ -f "$ELF" ]] || { echo "$ELF not built — run ./build.sh" >&2; exit 1; }
if [[ -n "${PS5_PAYLOAD_SDK:-}" && -x "${PS5_PAYLOAD_SDK}/bin/prospero-deploy" ]]; then
    exec "${PS5_PAYLOAD_SDK}/bin/prospero-deploy" -h "$HOST" -p "$PORT" "$ELF"
fi
# without the SDK's deploy tool: elfldr takes the raw ELF on the socket and echoes stdio back
exec nc -q 5 "$HOST" "$PORT" < "$ELF"
