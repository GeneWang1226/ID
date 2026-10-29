#!/usr/bin/env bash
# 签发临时凭证：把 kdbx 某条目的密码提取到 tmpfs，供 secret-exec.sh 代执行使用。
# 分级由条目 Tags 决定（在 KeePassXC 客户端打标，服务器端只读）:
#   ai-deny  拒绝签发（重要账号，只能 make pass 剪贴板或手动）
#   ai-auto  限时凭证，TTL 内可多次使用（默认 600 秒）
#   无标签   一次性凭证（默认 300 秒，用一次即销毁）
# 密码只写 tmpfs 文件（0600），不进日志、终端输出或命令行参数。
# 用法: grant-secret.sh <身份> <条目标题> [--ttl 秒]

set -euo pipefail

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

LOG_DIR="$BASE_DIR/logs"
mkdir -p "$LOG_DIR"
chmod 700 "$LOG_DIR"
log() { printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_DIR/scripts.log"; }

# 颜色（仅终端输出时生效；管道/重定向自动无色）
if [[ -t 1 ]]; then
    C_OK=$'\033[32m'; C_WARN=$'\033[33m'; C_HEAD=$'\033[1;36m'; C_DIM=$'\033[2m'; C_ERR=$'\033[1;31m'; C_N=$'\033[0m'
else
    C_OK=''; C_WARN=''; C_HEAD=''; C_DIM=''; C_ERR=''; C_N=''
fi

usage() {
    echo "用法: $(basename "$0") <身份> <条目标题> [--ttl 秒]" >&2
    exit 1
}

[[ $# -ge 2 ]] || usage
IDENTITY="$1"; ENTRY="$2"; shift 2
TTL=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --ttl) TTL="${2:-}"; shift 2 ;;
        *) usage ;;
    esac
done

KDBX="$BASE_DIR/identities/$IDENTITY/passwords/$IDENTITY.kdbx"
[[ -f "$KDBX" ]] || { printf '%s错误: 密码库不存在: %s%s\n' "$C_ERR" "$KDBX" "$C_N" >&2; exit 1; }

# 凭证目录放用户级 tmpfs，重启即清空
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
GRANT_DIR="$RUNTIME_DIR/id-grants"
mkdir -p "$GRANT_DIR"
chmod 700 "$GRANT_DIR"

# 清理已过期凭证
cleanup_expired() {
    local m exp
    for m in "$GRANT_DIR"/*.meta; do
        [[ -e "$m" ]] || continue
        exp="$(grep -E '^expires=' "$m" | head -n1 | cut -d= -f2)"
        if [[ -n "$exp" && "$exp" -le "$(date +%s)" ]]; then
            rm -f "$m" "${m%.meta}.secret"
        fi
    done
}
cleanup_expired

# 一次调用同时取 Tags 和 Password（只提示一次主密码，由 keepassxc-cli 直接读终端）
out=""
if ! out="$(keepassxc-cli show -q -s -a Tags -a Password "$KDBX" "$ENTRY")"; then
    printf '%s错误: 读取条目失败（主密码错误或条目不存在）: %s%s\n' "$C_ERR" "$ENTRY" "$C_N" >&2
    exit 1
fi

tags="${out%%$'\n'*}"          # 第一行是 Tags
password="${out#*$'\n'}"       # 其余是密码
[[ "$out" == *$'\n'* ]] || password=""
if [[ -z "$password" ]]; then
    printf '%s错误: 该条目没有密码字段，不能签发凭证%s\n' "$C_ERR" "$C_N" >&2
    exit 1
fi

# 解析级别：ai-deny 优先于 ai-auto；Tags 以 ; 或 , 分隔
level="ask"
tl=" ${tags//[;,]/ } "
if [[ "$tl" == *" ai-deny "* ]]; then
    level="deny"
elif [[ "$tl" == *" ai-auto "* ]]; then
    level="auto"
fi

if [[ "$level" == "deny" ]]; then
    log "grant-secret DENY $IDENTITY/$ENTRY"
    printf '%s错误: 条目标记为 ai-deny，拒绝签发。请用 make pass 复制剪贴板或手动输入。%s\n' "$C_ERR" "$C_N" >&2
    exit 1
fi

# TTL：ask 默认 300 秒，auto 默认 600 秒，上限 3600
default_ttl=300; [[ "$level" == "auto" ]] && default_ttl=600
TTL="${TTL:-$default_ttl}"
[[ "$TTL" =~ ^[0-9]+$ ]] || { printf '%s错误: --ttl 必须是正整数秒%s\n' "$C_ERR" "$C_N" >&2; exit 1; }
if [[ "$TTL" -gt 3600 ]]; then
    printf '%s警告: TTL 超过 3600 秒已截断为 3600%s\n' "$C_WARN" "$C_N" >&2
    TTL=3600
fi

token="$(openssl rand -hex 16)"
expires=$(( $(date +%s) + TTL ))
oneshot=0; [[ "$level" == "ask" ]] && oneshot=1

(
    umask 077
    printf '%s' "$password" > "$GRANT_DIR/$token.secret"
    printf 'identity=%s\nentry=%s\nlevel=%s\nexpires=%s\noneshot=%s\n' \
        "$IDENTITY" "$ENTRY" "$level" "$expires" "$oneshot" > "$GRANT_DIR/$token.meta"
)
unset password out

log "grant-secret $IDENTITY/$ENTRY level=$level ttl=$TTL"

printf '%s✓ 已签发临时凭证%s\n' "$C_OK" "$C_N"
printf '  %s条目:%s %s/%s\n' "$C_DIM" "$C_N" "$IDENTITY" "$ENTRY"
printf '  %s级别:%s %s（%s）\n' "$C_DIM" "$C_N" "$level" "$([[ "$level" == ask ]] && echo '用一次即销毁' || echo 'TTL 内可多次使用')"
printf '  %s过期:%s %s（%d 秒后）\n' "$C_DIM" "$C_N" "$(date -d "@$expires" '+%H:%M:%S')" "$TTL"
printf '  %stoken:%s %s\n' "$C_HEAD" "$C_N" "$token"
echo
printf '%sAI 使用方式（密码不出现明文）:%s\n' "$C_HEAD" "$C_N"
printf '  make use token=%s cmd='\''curl -u "user:$$ID_SECRET" https://...'\''\n' "$token"
printf '  %s（cmd 里用双引号、别用单引号；经 make 传参时 $ 要写成 $$）%s\n' "$C_DIM" "$C_N"
