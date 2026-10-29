#!/usr/bin/env bash
# 安全自检：ignore 规则、git 追踪、文件权限、明文机密扫描
# 用法: scripts/security-check.sh   （或 make check）
# 建议: 每次 commit / 开同步前跑一遍

set -uo pipefail
BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$BASE_DIR"

fail=0
ok()  { printf '  ✓ %s\n' "$1"; }
bad() { printf '  ✗ %s\n' "$1"; fail=1; }
sec() { printf '\n== %s ==\n' "$1"; }

# ===== 1. ignore 文件重复行 =====
check_dupes() {
    local f="$1"
    [[ -f "$f" ]] || { bad "$f 不存在"; return; }
    local dupes
    dupes=$(grep -vE '^\s*(#|//|$)' "$f" | sort | uniq -d)
    if [[ -n "$dupes" ]]; then
        bad "$f 有重复行:"
        printf '%s\n' "$dupes" | sed 's/^/      /'
    else
        ok "$f 无重复行"
    fi
}

sec "1. ignore 文件重复行"
check_dupes .gitignore
check_dupes .stignore

# ===== 2. git 不得追踪的敏感路径 =====
sec "2. git 追踪检查（这些绝不能入库）"
tracked="$(git ls-files)"
for pat in '^identities/' '^config$' '\.kdbx$' '\.turso\.env$' '^logs/' '^data/.*\.db$' '^backups/'; do
    hits="$(printf '%s\n' "$tracked" | grep -E "$pat" || true)"
    if [[ -n "$hits" ]]; then
        bad "被 git 追踪: $pat"
        printf '%s\n' "$hits" | sed 's/^/      /'
    else
        ok "未追踪: $pat"
    fi
done

# ===== 3. .gitignore 关键规则生效 =====
sec "3. .gitignore 规则生效"
for p in identities config logs data/id.db; do
    if git check-ignore -q "$p" 2>/dev/null; then
        ok "$p 被忽略"
    else
        bad "$p 未被 .gitignore 覆盖"
    fi
done
k="$(find identities -name '*.kdbx' 2>/dev/null | head -1)"
if [[ -n "$k" ]]; then
    git check-ignore -q "$k" && ok "$k 被忽略" || bad "$k 未被忽略"
fi

# ===== 4. .stignore 覆盖检查 =====
sec "4. .stignore 覆盖检查"
for line in /scripts /template /assets /docs /web /Makefile /docker-compose.yml \
            /AGENTS.md /README.md /.gitignore .git /logs /backups .turso.env; do
    # .stignore 可能带 (?d)/(?i) 前缀，匹配时去掉再比
    if sed 's/^(?[di])//g' .stignore | grep -qxF "$line"; then
        ok "$line"
    else
        bad ".stignore 缺少: $line"
    fi
done

# ===== 5. 文件权限 =====
sec "5. 文件权限"
perm() {
    [[ -e "$1" ]] || return 0
    local m
    m=$(stat -c '%a' "$1")
    [[ "$m" == "$2" ]] && ok "$1 权限 $m" || bad "$1 权限 $m，应为 $2"
}
perm config 600
perm logs 700
for d in identities/*/; do perm "${d%/}" 700; done
while IFS= read -r kdbx; do perm "$kdbx" 600; done < <(find identities -name '*.kdbx' 2>/dev/null)
while IFS= read -r envf; do perm "$envf" 600; done < <(find identities -name '.turso.env' 2>/dev/null)

# ===== 6. 追踪文件高危明文扫描 =====
sec "6. 追踪文件高危内容扫描"
hits="$(git grep -lE 'BEGIN [A-Z ]*PRIVATE KEY|api_key=sk-[A-Za-z0-9]|AUTH_TOKEN=ey[A-Za-z0-9]' -- . 2>/dev/null || true)"
if [[ -n "$hits" ]]; then
    bad "疑似机密写进了追踪文件:"
    printf '%s\n' "$hits" | sed 's/^/      /'
else
    ok "未发现私钥 / API key / token 明文"
fi

sec "结果"
if [[ "$fail" -eq 0 ]]; then
    echo "全部通过 ✓"
else
    echo "存在失败项，请先处理再提交/同步"
    exit 1
fi
