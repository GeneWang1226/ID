#!/usr/bin/env bash
# 为某个身份创建 Turso 数据库，并导入 data/schema.sql
# 用法: turso-setup.sh <身份名>
# 前置条件: 已安装 turso CLI 且已 turso auth login（见 ~/ID/docs/MAINTENANCE.md）
# 注意: 项目已改用本地 SQLite（data/id.db），此脚本仅保留备用

set -euo pipefail

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IDENTITIES_DIR="$BASE_DIR/identities"

# 操作日志（只记操作，绝不记秘密内容；token 不落日志）
LOG_DIR="$BASE_DIR/logs"
mkdir -p "$LOG_DIR"
chmod 700 "$LOG_DIR"
log() { printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_DIR/scripts.log"; }

usage() {
    echo "用法: $(basename "$0") <身份名>" >&2
    exit 1
}

[[ $# -eq 1 ]] || usage
NAME="$1"
IDENTITY_DIR="$IDENTITIES_DIR/$NAME"
SCHEMA="$IDENTITY_DIR/data/schema.sql"
DB_NAME="id-$NAME"

[[ -d "$IDENTITY_DIR" ]] || { echo "错误: 身份不存在: $IDENTITY_DIR" >&2; exit 1; }
[[ -f "$SCHEMA" ]] || { echo "错误: 找不到表结构文件: $SCHEMA" >&2; exit 1; }

if ! command -v turso >/dev/null 2>&1; then
    echo "错误: 未安装 turso CLI。安装方法见 ~/ID/docs/MAINTENANCE.md" >&2
    exit 1
fi

# turso 未登录时仍会退出码 0，并把错误写到标准输出。不能只靠 set -e。
require_turso_login() {
    local out
    out="$(turso auth whoami 2>&1 || true)"
    if [[ "$out" == *"not logged in"* ]]; then
        echo "错误: 尚未登录 Turso。请先在这台机器执行: turso auth login" >&2
        exit 1
    fi
}

reject_if_unusable() {
    local label="$1"
    local value="$2"
    if [[ -z "$value" || "$value" == *"not logged in"* || "$value" == *$'\n'* ]]; then
        echo "错误: 没有拿到${label}，未写入 .turso.env" >&2
        exit 1
    fi
}

require_turso_login

echo "==> 创建 Turso 数据库: $DB_NAME"
turso db create "$DB_NAME"

echo "==> 导入表结构"
turso db shell "$DB_NAME" < "$SCHEMA"

DB_URL="$(turso db show "$DB_NAME" --url)"
DB_TOKEN="$(turso db tokens create "$DB_NAME")"
reject_if_unusable "数据库地址" "$DB_URL"
reject_if_unusable "访问令牌" "$DB_TOKEN"

ENV_FILE="$IDENTITY_DIR/.turso.env"
cat > "$ENV_FILE" <<EOF
# Turso 连接信息（机密，勿外传，勿提交）
TURSO_DB_NAME=$DB_NAME
TURSO_URL=$DB_URL
TURSO_AUTH_TOKEN=$DB_TOKEN
EOF
chmod 600 "$ENV_FILE"

log "turso-setup $NAME db=$DB_NAME"

echo
echo "完成。连接信息已写入: $ENV_FILE （权限 600）"
echo
echo "日常使用:"
echo "  turso db shell $DB_NAME          # 交互式 SQL"
echo "  source $ENV_FILE                 # 在程序/脚本里用 TURSO_URL + TURSO_AUTH_TOKEN"
echo
echo "提醒: 此库只放非机密个人信息；密码/密钥永远放 passwords/$NAME.kdbx"
