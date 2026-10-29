#!/usr/bin/env bash
# 用临时凭证代执行命令：密码经 ID_SECRET 环境变量注入子进程，不进命令行参数；
# 子进程输出里出现的密码一律替换为 *** 后才显示。一次性凭证用完立即销毁。
# 用法: secret-exec.sh <token> -- <命令...>
# 例: secret-exec.sh ab12.. -- curl -u "user:$ID_SECRET" https://example.com

set -euo pipefail

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

LOG_DIR="$BASE_DIR/logs"
mkdir -p "$LOG_DIR"
chmod 700 "$LOG_DIR"
log() { printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_DIR/scripts.log"; }

if [[ -t 1 ]]; then
    C_ERR=$'\033[1;31m'; C_DIM=$'\033[2m'; C_N=$'\033[0m'
else
    C_ERR=''; C_DIM=''; C_N=''
fi

usage() {
    echo "用法: $(basename "$0") <token> -- <命令...>" >&2
    exit 1
}

[[ $# -ge 3 && "$2" == "--" ]] || usage
TOKEN="$1"; shift 2
CMD="$*"

[[ "$TOKEN" =~ ^[0-9a-f]{32}$ ]] || { printf '%s错误: token 格式不对%s\n' "$C_ERR" "$C_N" >&2; exit 1; }

RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
GRANT_DIR="$RUNTIME_DIR/id-grants"
META="$GRANT_DIR/$TOKEN.meta"
SECRET_FILE="$GRANT_DIR/$TOKEN.secret"

if [[ ! -f "$META" || ! -f "$SECRET_FILE" ]]; then
    printf '%s错误: 凭证不存在或已用完/撤销%s\n' "$C_ERR" "$C_N" >&2
    exit 1
fi

meta_get() { grep -E "^$1=" "$META" | head -n1 | cut -d= -f2-; }
identity="$(meta_get identity)"
entry="$(meta_get entry)"
expires="$(meta_get expires)"
oneshot="$(meta_get oneshot)"

now="$(date +%s)"
if [[ -z "$expires" || "$expires" -le "$now" ]]; then
    rm -f "$META" "$SECRET_FILE"
    printf '%s错误: 凭证已过期，请重新 make grant%s\n' "$C_ERR" "$C_N" >&2
    exit 1
fi

secret="$(cat "$SECRET_FILE")"
if [[ -z "$secret" ]]; then
    printf '%s错误: 凭证内容为空%s\n' "$C_ERR" "$C_N" >&2
    exit 1
fi

# 执行并捕获输出。密码只经环境变量传给子进程。
rc=0
out="$(ID_SECRET="$secret" bash -c "$CMD" 2>&1)" || rc=$?

# 一次性凭证：用完即焚
if [[ "$oneshot" == "1" ]]; then
    rm -f "$META" "$SECRET_FILE"
fi

# 输出打码：bash 的 ${var//pat/repl} 中 pat 是通配模式，先转义密码里的 \ * ? [
pat="${secret//\\/\\\\}"
pat="${pat//\*/\\*}"
pat="${pat//\?/\\?}"
pat="${pat//\[/\\[}"
unset secret
printf '%s\n' "${out//"$pat"/***}"

log "secret-exec ${TOKEN:0:8} $identity/$entry rc=$rc"
if [[ "$rc" -ne 0 ]]; then
    printf '%s（命令退出码 %d）%s\n' "$C_DIM" "$rc" "$C_N" >&2
fi
exit "$rc"
