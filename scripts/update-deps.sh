#!/usr/bin/env bash
# deps.lock の指定リポジトリ(省略時は RPoem)を origin の最新コミットへ更新し、
# fetch-deps.sh で .deps に反映する。更新後は cargo check / テストを通してからコミットする。
# 使い方: scripts/update-deps.sh [名前...]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
names=("$@"); [ ${#names[@]} -eq 0 ] && names=(RPoem)
for name in "${names[@]}"; do
  url=$(grep -E "^$name\s" "$root/deps.lock" | awk '{print $2}')
  [ -n "$url" ] || { echo "deps.lock に $name がありません" >&2; exit 1; }
  rev=$(git ls-remote "$url" HEAD | awk '{print $1}')
  sed -i -E "s|^($name\s+\S+\s+)[0-9a-f]{40}|\1$rev|" "$root/deps.lock"
  echo "更新: $name -> ${rev:0:12}"
done
bash "$root/scripts/fetch-deps.sh"
