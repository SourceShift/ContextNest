#!/usr/bin/env bash
set -euo pipefail
cn_header_file="${CONTEXTNEST_OPERATOR_HEADERS:-}"
if [[ -z "$cn_header_file" ]]; then
  for cn_argument in "$@"; do
    case "$cn_argument" in
      http://127.0.0.1:28080/*|http://localhost:28080/*) cn_header_file="$HOME/.contextnest/tenant-auth/operator.headers" ;;
    esac
  done
fi
cn_auth_args=()
if [[ -n "$cn_header_file" && -r "$cn_header_file" ]]; then
  cn_auth_args=(--header "@$cn_header_file")
fi
exec curl "${cn_auth_args[@]}" "$@"
