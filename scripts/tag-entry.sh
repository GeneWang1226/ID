#!/usr/bin/env bash
# 给 kdbx 条目打/改/清标签（keepassxc-cli 没有写 tag 的能力，这里用
# 导出 XML → 改标签 → 导入新库 → 原子替换 实现）。
# 临时 XML 含全部明文，只放用户级 tmpfs，用完即删；主密码只经 stdin 传递。
# 用法: tag-entry.sh <身份> <条目标题> <标签>
#   标签可多个，用英文分号分隔（例: "ai-auto;bot"）；传空字符串清除全部标签。

set -euo pipefail

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

LOG_DIR="$BASE_DIR/logs"
mkdir -p "$LOG_DIR"
chmod 700 "$LOG_DIR"
log() { printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_DIR/scripts.log"; }

if [[ -t 1 ]]; then
    C_OK=$'\033[32m'; C_ERR=$'\033[1;31m'; C_WARN=$'\033[33m'; C_DIM=$'\033[2m'; C_N=$'\033[0m'
else
    C_OK=''; C_ERR=''; C_WARN=''; C_DIM=''; C_N=''
fi

usage() {
    echo "用法: $(basename "$0") <身份> <条目标题> <标签（多个用;分隔，空串清除）>" >&2
    exit 1
}

[[ $# -eq 3 ]] || usage
IDENTITY="$1"; ENTRY="$2"; TAGS="$3"

KDBX="$BASE_DIR/identities/$IDENTITY/passwords/$IDENTITY.kdbx"
[[ -f "$KDBX" ]] || { printf '%s错误: 密码库不存在: %s%s\n' "$C_ERR" "$KDBX" "$C_N" >&2; exit 1; }

command -v python3 >/dev/null || { printf '%s错误: 需要 python3%s\n' "$C_ERR" "$C_N" >&2; exit 1; }

# 工作目录放用户级 tmpfs：临时 XML 是整库明文，绝不能落普通磁盘
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
mkdir -p "$RUNTIME_DIR"
TMPD="$(mktemp -d "$RUNTIME_DIR/tag-entry.XXXXXX")"
chmod 700 "$TMPD"
cleanup() { rm -rf "$TMPD"; }
trap cleanup EXIT

# 主密码只问一次，只经 stdin 传给 keepassxc-cli；终端交互提问，管道场景从 stdin 读
if [[ -t 0 ]]; then
    read -r -s -p "主密码: " MASTER
    echo
else
    read -r -s MASTER
fi
[[ -n "$MASTER" ]] || { printf '%s错误: 主密码不能为空%s\n' "$C_ERR" "$C_N" >&2; exit 1; }

XML="$TMPD/db.xml"
if ! printf '%s\n' "$MASTER" | keepassxc-cli export "$KDBX" 2>/dev/null > "$XML"; then
    unset MASTER
    printf '%s错误: 导出失败（主密码错误？）%s\n' "$C_ERR" "$C_N" >&2
    exit 1
fi
chmod 600 "$XML"

# 注入标签（按标题精确匹配条目；标题/标签做 XML 转义）
TAGS="$TAGS" ENTRY="$ENTRY" python3 - "$XML" <<'PYEOF'
import os, re, sys
from xml.sax.saxutils import escape

path, title, tags = sys.argv[1], os.environ["ENTRY"], os.environ["TAGS"]
xml = open(path, encoding="utf-8").read()
m = re.search(r"<Key>Title</Key>\s*<Value>%s</Value>" % re.escape(escape(title)), xml)
if not m:
    print("错误: 库里找不到条目: %s" % title, file=sys.stderr)
    sys.exit(1)
i = m.start()
t = escape(tags)
j = xml.rfind("<Tags/>", 0, i)
if j != -1:
    xml = xml[:j] + "<Tags>%s</Tags>" % t + xml[j + len("<Tags/>"):]
else:
    # 已有标签：替换内容
    j = xml.rfind("<Tags>", 0, i)
    k = xml.index("</Tags>", j)
    xml = xml[:j] + "<Tags>%s</Tags>" % t + xml[k + len("</Tags>"):]
open(path, "w", encoding="utf-8").write(xml)
PYEOF

# 导入为新库（同一主密码），验证标签后原子替换
NEWDBX="$TMPD/new.kdbx"
if ! printf '%s\n%s\n' "$MASTER" "$MASTER" | keepassxc-cli import -q -p "$XML" "$NEWDBX" 2>/dev/null; then
    unset MASTER
    printf '%s错误: 导入新库失败%s\n' "$C_ERR" "$C_N" >&2
    exit 1
fi

got="$(printf '%s\n' "$MASTER" | keepassxc-cli show -q -a Tags "$NEWDBX" "$ENTRY" 2>/dev/null || true)"
unset MASTER
# KeePassXC 可能把分隔符规范成逗号，按集合比较而不是逐字符
norm() { printf '%s' "$1" | tr ';,' '\n\n' | sed '/^ *$/d' | sed 's/^ *//;s/ *$//' | sort | tr '\n' ','; }
if [[ "$(norm "$got")" != "$(norm "$TAGS")" ]]; then
    printf '%s错误: 新库验证失败（读到标签 [%s]，期望 [%s]），原库未改动%s\n' "$C_ERR" "$got" "$TAGS" "$C_N" >&2
    exit 1
fi

chmod 600 "$NEWDBX"
mv "$NEWDBX" "$KDBX"
log "tag-entry $IDENTITY/$ENTRY tags=[$TAGS]"
printf '%s✓ 已更新标签:%s %s/%s → [%s]\n' "$C_OK" "$C_N" "$IDENTITY" "$ENTRY" "$TAGS"
printf '%s（新库经导入重建，条目/附件保留；原数据库级自定义设置如有需自行检查）%s\n' "$C_DIM" "$C_N"
