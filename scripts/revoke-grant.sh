#!/usr/bin/env bash
# 撤销或查看临时凭证。
# 用法:
#   revoke-grant.sh --list      列出活跃凭证（默认动作，只读元数据，不显示秘密）
#   revoke-grant.sh <token>     撤销指定凭证
#   revoke-grant.sh --all       撤销全部凭证

set -euo pipefail

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

LOG_DIR="$BASE_DIR/logs"
mkdir -p "$LOG_DIR"
chmod 700 "$LOG_DIR"
log() { printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_DIR/scripts.log"; }

if [[ -t 1 ]]; then
    C_OK=$'\033[32m'; C_ERR=$'\033[1;31m'; C_HEAD=$'\033[1;36m'; C_DIM=$'\033[2m'; C_N=$'\033[0m'
else
    C_OK=''; C_ERR=''; C_HEAD=''; C_DIM=''; C_N=''
fi

RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
GRANT_DIR="$RUNTIME_DIR/id-grants"
mkdir -p "$GRANT_DIR"
chmod 700 "$GRANT_DIR"

meta_get() { grep -E "^$2=" "$1" | head -n1 | cut -d= -f2-; }

now="$(date +%s)"

list_grants() {
    local m exp identity entry level oneshot found=0
    for m in "$GRANT_DIR"/*.meta; do
        [[ -e "$m" ]] || continue
        exp="$(meta_get "$m" expires)"
        if [[ -z "$exp" || "$exp" -le "$now" ]]; then
            rm -f "$m" "${m%.meta}.secret"   # 顺手清理过期凭证
            continue
        fi
        found=1
        identity="$(meta_get "$m" identity)"
        entry="$(meta_get "$m" entry)"
        level="$(meta_get "$m" level)"
        oneshot="$(meta_get "$m" oneshot)"
        printf '  %s%s%s  %s/%s  %s（%s）  %s剩余 %d 秒%s\n' \
            "$C_HEAD" "$(basename "$m" .meta)" "$C_N" \
            "$identity" "$entry" "$level" "$([[ "$oneshot" == 1 ]] && echo '一次性' || echo '多次')" \
            "$C_DIM" "$(( exp - now ))" "$C_N"
    done
    if [[ "$found" -eq 0 ]]; then
        printf '%s没有活跃凭证%s\n' "$C_DIM" "$C_N"
    fi
}

case "${1:---list}" in
    --list|-l|"")
        printf '%s== 活跃凭证 ==%s\n' "$C_HEAD" "$C_N"
        list_grants
        ;;
    --all)
        rm -f "$GRANT_DIR"/*.secret "$GRANT_DIR"/*.meta 2>/dev/null || true
        log "revoke-grant --all"
        printf '%s✓ 已撤销全部凭证%s\n' "$C_OK" "$C_N"
        ;;
    *)
        TOKEN="$1"
        [[ "$TOKEN" =~ ^[0-9a-f]{32}$ ]] || { printf '%s错误: token 格式不对%s\n' "$C_ERR" "$C_N" >&2; exit 1; }
        if [[ -f "$GRANT_DIR/$TOKEN.meta" || -f "$GRANT_DIR/$TOKEN.secret" ]]; then
            rm -f "$GRANT_DIR/$TOKEN.meta" "$GRANT_DIR/$TOKEN.secret"
            log "revoke-grant ${TOKEN:0:8}"
            printf '%s✓ 已撤销凭证 %s%s\n' "$C_OK" "$TOKEN" "$C_N"
        else
            printf '%s错误: 凭证不存在（可能已过期或用尽）%s\n' "$C_ERR" "$C_N" >&2
            exit 1
        fi
        ;;
esac
