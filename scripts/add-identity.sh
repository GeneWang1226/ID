#!/usr/bin/env bash
# 新增一个身份：把 template/ 复制为 identities/<name>/
# 如果身份已存在，则进入补齐模式：只复制缺失的内容，绝不覆盖已有文件。
# 用法: add-identity.sh <身份名>

set -euo pipefail

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE_DIR="$BASE_DIR/template"
IDENTITIES_DIR="$BASE_DIR/identities"
SECRETS_FILE="$BASE_DIR/config/secrets/.secrets.env"
LEGACY_CONFIG_FILE="$BASE_DIR/docker/.env"

# 操作日志（只记操作，绝不记秘密内容）
LOG_DIR="$BASE_DIR/logs"
mkdir -p "$LOG_DIR"
chmod 700 "$LOG_DIR"
log() { printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_DIR/scripts.log"; }

# 颜色（仅终端输出时生效；管道/重定向自动无色）
if [[ -t 1 ]]; then
    C_OK=$'\033[32m'; C_WARN=$'\033[33m'; C_HEAD=$'\033[1;36m'; C_DIM=$'\033[2m'; C_N=$'\033[0m'
else
    C_OK=''; C_WARN=''; C_HEAD=''; C_DIM=''; C_N=''
fi

usage() {
    echo "用法: $(basename "$0") <身份名>" >&2
    echo "  身份名只允许: 小写字母、数字、连字符、下划线（例: work, personal-2）" >&2
    exit 1
}

# 从 config/secrets/.secrets.env 读取新建密码库的初始主密码。调用方不得把返回值写入日志或终端。
read_kdbx_initial_password() {
    local config_file="$SECRETS_FILE"
    local key='KDBX_INITIAL_PASSWORD'
    if [[ ! -f "$config_file" ]]; then
        # 兼容旧安装；新配置请迁移到 config/secrets/.secrets.env。
        config_file="$LEGACY_CONFIG_FILE"
        key='kdbx_initial_password'
    fi
    if [[ ! -f "$config_file" ]]; then
        echo "错误: 找不到 $SECRETS_FILE（模板见 config/secrets/.secrets.env.example）" >&2
        exit 1
    fi
    local mode
    mode="$(stat -c '%a' "$config_file")"
    if [[ "$mode" != "600" ]]; then
        echo "错误: $config_file 权限必须是 600（当前 $mode）" >&2
        exit 1
    fi
    local line val
    line="$(grep -E "^${key}=" "$config_file" | head -n 1 || true)"
    if [[ -z "$line" ]]; then
        echo "错误: $config_file 缺少 $key" >&2
        exit 1
    fi
    val="${line#"$key"=}"
    if [[ ${#val} -ge 2 && ( "$val" == \"*\" || "$val" == \'*\' ) ]]; then
        val="${val:1:${#val}-2}"
    fi
    if [[ -z "$val" ]]; then
        echo "错误: $key 为空" >&2
        exit 1
    fi
    printf '%s' "$val"
}

# 用 config/secrets/.secrets.env 里的初始密码新建空白库。密码只经 stdin 传给 KeePassXC，不进参数、日志或输出。
create_kdbx() {
    local dest="$1"
    local password err
    password="$(read_kdbx_initial_password)"
    err="$(mktemp)"
    if ! printf '%s\n%s\n' "$password" "$password" | keepassxc-cli db-create -q -p "$dest" 2>"$err"; then
        echo "错误: 创建密码库失败: $dest" >&2
        cat "$err" >&2
        rm -f "$err"
        exit 1
    fi
    rm -f "$err"
    chmod 600 "$dest"
}

[[ $# -eq 1 ]] || usage
NAME="$1"

if [[ ! "$NAME" =~ ^[a-z0-9][a-z0-9_-]*$ ]]; then
    echo "错误: 身份名 '$NAME' 不合法（小写字母/数字开头，只允许 a-z 0-9 - _）" >&2
    exit 1
fi

if [[ ! -d "$TEMPLATE_DIR" ]]; then
    echo "错误: 模板目录不存在: $TEMPLATE_DIR" >&2
    exit 1
fi

TARGET="$IDENTITIES_DIR/$NAME"
KDBX="$TARGET/passwords/$NAME.kdbx"
ADDED=()

if [[ -e "$TARGET" ]]; then
    # ===== 补齐模式：身份已存在，只补缺失内容 =====
    MODE="backfill"
    chmod 700 "$TARGET"

    # 逐条比对模板（父目录先于子目录处理），缺失才复制
    while IFS= read -r -d '' entry; do
        rel="${entry#"$TEMPLATE_DIR"/}"
        [[ "$rel" == "passwords/template.kdbx" ]] && continue  # kdbx 单独处理
        if [[ ! -e "$TARGET/$rel" ]]; then
            cp -a "$entry" "$TARGET/$rel"
            ADDED+=("$rel")
        fi
    done < <(find "$TEMPLATE_DIR" -mindepth 1 -print0)
else
    # ===== 新建模式 =====
    MODE="create"
    cp -a "$TEMPLATE_DIR" "$TARGET"
    chmod 700 "$TARGET"
fi

# ===== 密码库：缺失时用 config/secrets/.secrets.env 里的初始密码新建，绝不覆盖已有库 =====
mkdir -p "$TARGET/passwords"
CREATED_KDBX=0
if [[ ! -f "$KDBX" ]]; then
    rm -f "$TARGET/passwords/template.kdbx"
    create_kdbx "$KDBX"
    CREATED_KDBX=1
    [[ "$MODE" == "backfill" ]] && ADDED+=("passwords/$NAME.kdbx")
elif [[ -f "$TARGET/passwords/template.kdbx" ]]; then
    echo "提示: $TARGET/passwords/template.kdbx 是多余的空白模板，确认 $NAME.kdbx 正常后可手动删除。"
fi

if [[ "$MODE" == "create" ]]; then
    log "add-identity create $NAME"
    printf '%s已创建身份目录:%s %s\n' "$C_OK" "$C_N" "$TARGET"
    echo
    printf '%s结构:%s\n' "$C_HEAD" "$C_N"
    find "$TARGET" -mindepth 1 | sort | sed "s|$TARGET/||"
else
    if [[ ${#ADDED[@]} -eq 0 ]]; then
        log "add-identity backfill $NAME: 无需补齐"
        echo "身份 '$NAME' 已存在，且模板内容齐全，无需补齐。"
    else
        log "add-identity backfill $NAME: ${ADDED[*]}"
        echo "身份 '$NAME' 已存在，已补齐缺失内容:"
        printf '  + %s\n' "${ADDED[@]}"
    fi
fi

echo
printf '%s下一步建议:%s\n' "$C_HEAD" "$C_N"
echo "  1. 填写 $TARGET/profile.md（身份档案与喜好）"
if [[ "$CREATED_KDBX" -eq 1 ]]; then
    printf '  2. %s⚠️  密码库已用 config/secrets/.secrets.env 里的初始密码创建，请尽快改成这个身份自己的主密码:%s\n' "$C_WARN" "$C_N"
    echo "       keepassxc-cli db-edit --set-password $KDBX"
    printf '     %s第一条提示输入的是 config/secrets/.secrets.env 中的初始密码，之后两遍输入新密码。%s\n' "$C_DIM" "$C_N"
else
    echo "  2. 密码库已存在，本次未改动。"
fi
echo "  3. 需要 Turso 存个人信息时运行: ~/ID/scripts/turso-setup.sh $NAME"
echo "  4. 如有身份专属规则，补充 $TARGET/AGENTS.md"
