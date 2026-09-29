#!/usr/bin/env bash
# ThreadType 起動スクリプト
# 使い方:
#   ./run.sh          リリースビルドで起動（初回はビルドに時間がかかる）
#   ./run.sh --debug  デバッグビルドで起動（ビルドが速い）
#   ./run.sh --test   テストだけ実行
set -euo pipefail
cd "$(dirname "$0")"

case "${1:-}" in
  --test)
    exec cargo test
    ;;
  --debug)
    exec cargo run --bin threadtype
    ;;
  *)
    exec cargo run --release --bin threadtype
    ;;
esac
