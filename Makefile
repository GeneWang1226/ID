# ID 项目常用操作。用法: make <目标> [变量=值]（变量一律小写）
# 所有目标的参数都缺省可交互输入。例:
#   make show id=gene            直接执行
#   make item id=gene            逐个询问分类/名称/内容/备注
#   make pass-add id=gene        询问标题、用户名，再提示输主密码

DB := data/id.db

.PHONY: help new passwd pass pass-show pass-list pass-add pass-add-manual grant use grants revoke web web-stop web-logs db db-init show item event status backup

help: ## 显示本帮助
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  make %-16s %s\n", $$1, $$2}'

new: ## 新增身份（id=gene）
	@v="$(id)"; [ -n "$$v" ] || read -p "身份名: " v; \
	test -n "$$v" || { echo "错误: 身份名不能为空"; exit 1; }; \
	scripts/add-identity.sh "$$v"

passwd: ## 修改某身份 kdbx 主密码（id=gene）
	@v="$(id)"; [ -n "$$v" ] || read -p "身份名: " v; \
	test -n "$$v" || { echo "错误: 身份名不能为空"; exit 1; }; \
	keepassxc-cli db-edit --set-password "identities/$$v/passwords/$$v.kdbx"

pass: ## 复制密码到剪贴板，10 秒后自动清空（id=gene title="..."）
	@iv="$(id)";    [ -n "$$iv" ] || read -p "身份: " iv; \
	tv="$(title)"; [ -n "$$tv" ] || read -p "条目标题: " tv; \
	test -n "$$iv" -a -n "$$tv" || { echo "错误: 身份和标题不能为空"; exit 1; }; \
	keepassxc-cli clip "identities/$$iv/passwords/$$iv.kdbx" "$$tv"

pass-show: ## 显示整条记录（id=gene title="..."）
	@iv="$(id)";    [ -n "$$iv" ] || read -p "身份: " iv; \
	tv="$(title)"; [ -n "$$tv" ] || read -p "条目标题: " tv; \
	test -n "$$iv" -a -n "$$tv" || { echo "错误: 身份和标题不能为空"; exit 1; }; \
	keepassxc-cli show "identities/$$iv/passwords/$$iv.kdbx" "$$tv"

pass-list: ## 列出某身份 kdbx 里所有条目（id=gene）
	@iv="$(id)"; [ -n "$$iv" ] || read -p "身份: " iv; \
	test -n "$$iv" || { echo "错误: 身份不能为空"; exit 1; }; \
	keepassxc-cli ls "identities/$$iv/passwords/$$iv.kdbx"

pass-add: ## 新增条目并自动生成强密码（id=gene，缺省逐个询问）
	@iv="$(id)";    [ -n "$$iv" ]    || read -p "身份: " iv; \
	tv="$(title)"; [ -n "$$tv" ]    || read -p "条目标题: " tv; \
	uv="$(user)";  [ -n "$$uv" ]    || read -p "用户名(可空): " uv; \
	test -n "$$iv" -a -n "$$tv" || { echo "错误: 身份和标题不能为空"; exit 1; }; \
	keepassxc-cli add -g -L 20 -l -U -n -s --every-group --exclude-similar -u "$$uv" "identities/$$iv/passwords/$$iv.kdbx" "$$tv"

pass-add-manual: ## 新增条目，手动输入密码（id=gene，缺省逐个询问）
	@iv="$(id)";    [ -n "$$iv" ]    || read -p "身份: " iv; \
	tv="$(title)"; [ -n "$$tv" ]    || read -p "条目标题: " tv; \
	uv="$(user)";  [ -n "$$uv" ]    || read -p "用户名(可空): " uv; \
	test -n "$$iv" -a -n "$$tv" || { echo "错误: 身份和标题不能为空"; exit 1; }; \
	keepassxc-cli add -p -u "$$uv" "identities/$$iv/passwords/$$iv.kdbx" "$$tv"

grant: ## 签发临时凭证给 AI 用（id=gene title="..." ttl=600，缺省逐个询问）
	@iv="$(id)";    [ -n "$$iv" ] || read -p "身份: " iv; \
	tv="$(title)"; [ -n "$$tv" ] || read -p "条目标题: " tv; \
	test -n "$$iv" -a -n "$$tv" || { echo "错误: 身份和标题不能为空" >&2; exit 1; }; \
	scripts/grant-secret.sh "$$iv" "$$tv" $(if $(ttl),--ttl $(ttl),)

use: ## 用凭证代执行命令（token=... cmd='curl -u "u:$$ID_SECRET" url'；cmd 里用双引号、别用单引号，$ 写 $$）
	@tv="$(token)"; [ -n "$$tv" ] || read -p "token: " tv; \
	test -n "$$tv" || { echo "错误: token 不能为空" >&2; exit 1; }; \
	cv='$(cmd)'; [ -n "$$cv" ] || read -p "命令: " cv; \
	test -n "$$cv" || { echo "错误: 命令不能为空" >&2; exit 1; }; \
	scripts/secret-exec.sh "$$tv" -- "$$cv"

grants: ## 列出活跃临时凭证
	@scripts/revoke-grant.sh --list

revoke: ## 撤销凭证（token=... 或 all=1）
	@if [ "$(all)" = "1" ]; then scripts/revoke-grant.sh --all; \
	else tv="$(token)"; [ -n "$$tv" ] || read -p "token: " tv; \
	test -n "$$tv" || { echo "错误: token 不能为空（或用 all=1）" >&2; exit 1; }; \
	scripts/revoke-grant.sh "$$tv"; fi

web: ## 构建并启动 Web 界面（http://localhost:3000）
	docker compose up -d --build

web-stop: ## 停止 Web 界面
	docker compose down

web-logs: ## 查看 Web 日志
	docker compose logs -f web

# 表结构是幂等的（IF NOT EXISTS），每次读写前跑一遍确保库已初始化
db-init:
	@mkdir -p data
	@sqlite3 $(DB) < template/data/schema.sql

db: db-init ## 打开 SQLite 交互式查询
	sqlite3 $(DB)

show: db-init ## 打印某身份的全部信息（id=gene）
	@iv="$(id)"; [ -n "$$iv" ] || read -p "身份: " iv; \
	test -n "$$iv" || { echo "错误: 身份不能为空"; exit 1; }; \
	echo "== $$iv 的条目 =="; \
	sqlite3 -header -column $(DB) "SELECT category AS '分类', key AS '名称', value AS '内容', note AS '备注', updated_at AS '更新时间' FROM items WHERE identity='$$iv' ORDER BY category, key;"; \
	echo; echo "== $$iv 的事件 =="; \
	sqlite3 -header -column $(DB) "SELECT happened_at AS '时间', title AS '标题', detail AS '详情' FROM events WHERE identity='$$iv' ORDER BY happened_at DESC;"

item: db-init ## 写/改条目（id=gene，缺省逐个询问；值里别用单引号）
	@iv="$(id)";     [ -n "$$iv" ] || read -p "身份: " iv; \
	cv="$(cat)";    [ -n "$$cv" ] || read -p "分类(account/address/preference/id-info/contact): " cv; \
	kv="$(key)";    [ -n "$$kv" ] || read -p "名称: " kv; \
	vv="$(value)";  [ -n "$$vv" ] || read -p "内容: " vv; \
	nv="$(note)";   [ -n "$$nv" ] || read -p "备注(可空): " nv; \
	test -n "$$iv" -a -n "$$cv" -a -n "$$kv" -a -n "$$vv" || { echo "错误: 身份/分类/名称/内容不能为空"; exit 1; }; \
	sqlite3 $(DB) "INSERT INTO items(identity,category,key,value,note) VALUES('$$iv','$$cv','$$kv','$$vv','$$nv') ON CONFLICT(identity,category,key) DO UPDATE SET value=excluded.value, note=excluded.note, updated_at=datetime('now');"

event: db-init ## 记事件（id=gene，缺省逐个询问；值里别用单引号）
	@iv="$(id)";    [ -n "$$iv" ] || read -p "身份: " iv; \
	tv="$(title)"; [ -n "$$tv" ] || read -p "标题: " tv; \
	dv="$(detail)"; [ -n "$$dv" ] || read -p "详情(可空): " dv; \
	test -n "$$iv" -a -n "$$tv" || { echo "错误: 身份和标题不能为空"; exit 1; }; \
	sqlite3 $(DB) "INSERT INTO events(identity,title,detail) VALUES('$$iv','$$tv','$$dv');"

status: db-init ## 总览：身份列表、各身份条目数、Web 运行状态
	@echo "== 身份 =="; ls identities/
	@echo; echo "== 条目数 =="; sqlite3 $(DB) "SELECT identity, COUNT(*) FROM items GROUP BY identity;"
	@echo; echo "== Web =="; docker compose ps 2>/dev/null || true

backup: ## 备份 SQLite 库到 backups/
	@mkdir -p backups
	cp $(DB) backups/id-$$(date +%Y%m%d-%H%M%S).db
	@ls -t backups/ | head -3
