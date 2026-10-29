# ID 项目常用操作。用法: make <目标> [变量=值]（变量一律小写）
# 所有目标的参数都缺省可交互输入。例:
#   make show id=gene            直接执行
#   make item id=gene            逐个询问分类/名称/内容/备注
#   make pass-add id=gene        询问标题、用户名，再提示输主密码

DB := data/id.db
COMPOSE := docker compose -f docker/docker-compose.yml $(if $(wildcard docker/.env),--env-file docker/.env,)

# 颜色（仅终端输出时生效；管道/重定向自动无色）
# 注意: $(shell) 的 stdout 被 make 捕获，所以要用 stderr 判断终端
TTY := $(shell [ -t 2 ] && echo 1)
ifeq ($(TTY),1)
H  := \033[1;36m
OK := \033[32m
ER := \033[1;31m
WN := \033[33m
DM := \033[2m
PR := \033[36m
N  := \033[0m
endif

.PHONY: help check settings new passwd pass pass-show pass-list pass-add pass-add-manual grant use grants revoke tag web web-stop web-logs db db-init show item event ai status backup

help: ## 显示本帮助
	@e=""; [ -t 1 ] && e=$$(printf '\033'); \
	grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk -v e="$$e" 'BEGIN {FS = ":.*?## "} { \
		if (e == "") printf "  make %-16s %s\n", $$1, $$2; \
		else printf "  %s[32mmake %-16s%s[0m %s[2m%s%s[0m\n", e, $$1, e, e, $$2, e }'

check: ## 安全自检（ignore 规则/追踪/权限/明文扫描），提交或同步前跑一遍
	@scripts/security-check.sh

settings: ## 方向键修改 AI 设置，并可隐藏输入 API key
	@scripts/settings.py

new: ## 新增身份（id=gene）
	@v="$(id)"; [ -n "$$v" ] || read -p "$$(printf '$(PR)身份名: $(N)')" v; \
	[ -n "$$v" ] || { printf '$(ER)错误: 身份名不能为空$(N)\n' >&2; exit 1; }; \
	scripts/add-identity.sh "$$v"

passwd: ## 修改某身份 kdbx 主密码（id=gene）
	@v="$(id)"; [ -n "$$v" ] || v="$$(scripts/select-identity.sh)"; \
	[ -n "$$v" ] || { printf '$(ER)错误: 身份名不能为空$(N)\n' >&2; exit 1; }; \
	keepassxc-cli db-edit --set-password "identities/$$v/passwords/$$v.kdbx"

pass: ## 复制密码到剪贴板，10 秒后自动清空（id=gene title="..."）
	@iv="$(id)";    [ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	tv="$(title)"; [ -n "$$tv" ] || read -p "$$(printf '$(PR)条目标题: $(N)')" tv; \
	[ -n "$$iv" -a -n "$$tv" ] || { printf '$(ER)错误: 身份和标题不能为空$(N)\n' >&2; exit 1; }; \
	keepassxc-cli clip "identities/$$iv/passwords/$$iv.kdbx" "$$tv"

pass-show: ## 显示整条记录（id=gene title="..."）
	@iv="$(id)";    [ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	tv="$(title)"; [ -n "$$tv" ] || read -p "$$(printf '$(PR)条目标题: $(N)')" tv; \
	[ -n "$$iv" -a -n "$$tv" ] || { printf '$(ER)错误: 身份和标题不能为空$(N)\n' >&2; exit 1; }; \
	keepassxc-cli show "identities/$$iv/passwords/$$iv.kdbx" "$$tv"

pass-list: ## 列出某身份 kdbx 里所有条目（id=gene）
	@iv="$(id)"; [ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	[ -n "$$iv" ] || { printf '$(ER)错误: 身份不能为空$(N)\n' >&2; exit 1; }; \
	keepassxc-cli ls "identities/$$iv/passwords/$$iv.kdbx"

pass-add: ## 新增条目并自动生成强密码（id=gene，缺省逐个询问）
	@iv="$(id)";    [ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	tv="$(title)"; [ -n "$$tv" ] || read -p "$$(printf '$(PR)条目标题: $(N)')" tv; \
	uv="$(user)";  [ -n "$$uv" ] || read -p "$$(printf '$(PR)用户名(可空): $(N)')" uv; \
	[ -n "$$iv" -a -n "$$tv" ] || { printf '$(ER)错误: 身份和标题不能为空$(N)\n' >&2; exit 1; }; \
	keepassxc-cli add -g -L 20 -l -U -n -s --every-group --exclude-similar -u "$$uv" "identities/$$iv/passwords/$$iv.kdbx" "$$tv"

pass-add-manual: ## 新增条目，手动输入密码（id=gene title="..."，缺省逐个询问）
	@iv="$(id)";    [ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	tv="$(title)"; [ -n "$$tv" ] || read -p "$$(printf '$(PR)条目标题: $(N)')" tv; \
	uv="$(user)";  [ -n "$$uv" ] || read -p "$$(printf '$(PR)用户名(可空): $(N)')" uv; \
	[ -n "$$iv" -a -n "$$tv" ] || { printf '$(ER)错误: 身份和标题不能为空$(N)\n' >&2; exit 1; }; \
	keepassxc-cli add -p -u "$$uv" "identities/$$iv/passwords/$$iv.kdbx" "$$tv"

grant: ## 签发临时凭证给 AI 用（id=gene title="..." ttl=600，缺省逐个询问）
	@iv="$(id)";    [ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	tv="$(title)"; [ -n "$$tv" ] || read -p "$$(printf '$(PR)条目标题: $(N)')" tv; \
	[ -n "$$iv" -a -n "$$tv" ] || { printf '$(ER)错误: 身份和标题不能为空$(N)\n' >&2; exit 1; }; \
	scripts/grant-secret.sh "$$iv" "$$tv" $(if $(ttl),--ttl $(ttl),)

use: ## 用凭证代执行命令（token=... cmd='curl -u "u:$$ID_SECRET" url'；cmd 里用双引号、别用单引号，$ 写 $$）
	@tv="$(token)"; [ -n "$$tv" ] || read -p "$$(printf '$(PR)token: $(N)')" tv; \
	[ -n "$$tv" ] || { printf '$(ER)错误: token 不能为空$(N)\n' >&2; exit 1; }; \
	cv='$(cmd)'; [ -n "$$cv" ] || read -p "$$(printf '$(PR)命令: $(N)')" cv; \
	[ -n "$$cv" ] || { printf '$(ER)错误: 命令不能为空$(N)\n' >&2; exit 1; }; \
	scripts/secret-exec.sh "$$tv" -- "$$cv"

grants: ## 列出活跃临时凭证
	@scripts/revoke-grant.sh --list

revoke: ## 撤销凭证（token=... 或 all=1）
	@if [ "$(all)" = "1" ]; then scripts/revoke-grant.sh --all; \
	else tv="$(token)"; [ -n "$$tv" ] || read -p "$$(printf '$(PR)token: $(N)')" tv; \
	[ -n "$$tv" ] || { printf '$(ER)错误: token 不能为空（或用 all=1）$(N)\n' >&2; exit 1; }; \
	scripts/revoke-grant.sh "$$tv"; fi

tag: ## 给 kdbx 条目打/改/清标签（id=gene title="..." tags=ai-deny；tags= 空则清除）
	@iv="$(id)";    [ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	tv="$(title)"; [ -n "$$tv" ] || read -p "$$(printf '$(PR)条目标题: $(N)')" tv; \
	[ -n "$$iv" -a -n "$$tv" ] || { printf '$(ER)错误: 身份和标题不能为空$(N)\n' >&2; exit 1; }; \
	gv='$(tags)'; [ -n "$$gv" ] || read -p "$$(printf '$(PR)标签(多个用;分隔，直接回车清除): $(N)')" gv; \
	scripts/tag-entry.sh "$$iv" "$$tv" "$$gv"

web: ## 构建并启动 Web 界面（http://localhost:3000）
	@set -a; [ ! -f config/secrets/.secrets.env ] || . ./config/secrets/.secrets.env; set +a; $(COMPOSE) up -d --build
	@printf '$(OK)Web 已启动: http://localhost:3000$(N)\n'

web-stop: ## 停止 Web 界面
	$(COMPOSE) down

web-logs: ## 查看 Web 日志
	$(COMPOSE) logs -f web

# 表结构是幂等的（IF NOT EXISTS），每次读写前跑一遍确保库已初始化
db-init:
	@mkdir -p data
	@sqlite3 $(DB) < template/data/schema.sql

db: db-init ## 打开 SQLite 交互式查询
	sqlite3 $(DB)

show: db-init ## 打印某身份的全部信息（id=gene）
	@iv="$(id)"; [ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	[ -n "$$iv" ] || { printf '$(ER)错误: 身份不能为空$(N)\n' >&2; exit 1; }; \
	printf '$(H)== %s 的条目 ==$(N)\n' "$$iv"; \
	sqlite3 -header -column $(DB) "SELECT category AS '分类', key AS '名称', value AS '内容', note AS '备注', updated_at AS '更新时间' FROM items WHERE identity='$$iv' ORDER BY category, key;"; \
	echo; printf '$(H)== %s 的事件 ==$(N)\n' "$$iv"; \
	sqlite3 -header -column $(DB) "SELECT happened_at AS '时间', title AS '标题', detail AS '详情' FROM events WHERE identity='$$iv' ORDER BY happened_at DESC;"

item: db-init ## 写/改条目（id=gene，缺省逐个询问；值里别用单引号）
	@iv="$(id)";     [ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	cv="$(cat)";    [ -n "$$cv" ] || read -p "$$(printf '$(PR)分类(account/address/preference/id-info/contact): $(N)')" cv; \
	kv="$(key)";    [ -n "$$kv" ] || read -p "$$(printf '$(PR)名称: $(N)')" kv; \
	vv="$(value)";  [ -n "$$vv" ] || read -p "$$(printf '$(PR)内容: $(N)')" vv; \
	nv="$(note)";   [ -n "$$nv" ] || read -p "$$(printf '$(PR)备注(可空): $(N)')" nv; \
	[ -n "$$iv" -a -n "$$cv" -a -n "$$kv" -a -n "$$vv" ] || { printf '$(ER)错误: 身份/分类/名称/内容不能为空$(N)\n' >&2; exit 1; }; \
	sqlite3 $(DB) "INSERT INTO items(identity,category,key,value,note) VALUES('$$iv','$$cv','$$kv','$$vv','$$nv') ON CONFLICT(identity,category,key) DO UPDATE SET value=excluded.value, note=excluded.note, updated_at=datetime('now');" && \
	printf '$(OK)✓ 已写入 %s/%s$(N)\n' "$$iv" "$$kv"

event: db-init ## 记事件（id=gene，缺省逐个询问；值里别用单引号）
	@iv="$(id)";     [ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	tv="$(title)";  [ -n "$$tv" ] || read -p "$$(printf '$(PR)标题: $(N)')" tv; \
	dv="$(detail)"; [ -n "$$dv" ] || read -p "$$(printf '$(PR)详情(可空): $(N)')" dv; \
	[ -n "$$iv" -a -n "$$tv" ] || { printf '$(ER)错误: 身份和标题不能为空$(N)\n' >&2; exit 1; }; \
	sqlite3 $(DB) "INSERT INTO events(identity,title,detail) VALUES('$$iv','$$tv','$$dv');" && \
	printf '$(OK)✓ 已记录: %s$(N)\n' "$$tv"

ai: ## AI 整理资料（↑/↓选身份；空行提交；结果后可执行命令或继续提问；设置见 config/config.toml）
	@iv="$(id)"; fv="$(file)"; \
	[ -n "$$iv" ] || iv="$$(scripts/select-identity.sh)"; \
	[ -n "$$iv" ] || { printf '$(ER)错误: 身份不能为空$(N)\n' >&2; exit 1; }; \
	if [ -n "$$fv" ]; then scripts/ai-organize.py --identity "$$iv" "$$fv"; \
	else scripts/ai-organize.py --identity "$$iv"; fi

status: db-init ## 总览：身份列表、各身份条目数、Web 运行状态
	@printf '$(H)== 身份 ==$(N)\n'; ls identities/
	@echo; printf '$(H)== 条目数 ==$(N)\n'; sqlite3 $(DB) "SELECT identity, COUNT(*) FROM items GROUP BY identity;"
	@echo; printf '$(H)== Web ==$(N)\n'; $(COMPOSE) ps 2>/dev/null || true

backup: ## 备份 SQLite 库到 backups/
	@mkdir -p backups
	cp $(DB) backups/id-$$(date +%Y%m%d-%H%M%S).db
	@printf '$(OK)✓ 已备份:$(N)\n'; ls -t backups/ | head -3
