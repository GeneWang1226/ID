# ID 项目常用操作。用法: make <目标> [变量=值]（变量一律小写）
# 例: make new name=gene
#     make item identity=gene cat=account key=proton-email value="gene@proton.me"

DB := data/id.db

.PHONY: help new passwd pass pass-show pass-add pass-add-manual web web-stop web-logs db db-init show item event status backup

help: ## 显示本帮助
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  make %-16s %s\n", $$1, $$2}'

new: ## 新增身份（name=gene）
	@test -n "$(name)" || { echo "用法: make new name=<身份名>"; exit 1; }
	scripts/add-identity.sh $(name)

passwd: ## 修改某身份 kdbx 主密码（name=gene）
	@test -n "$(name)" || { echo "用法: make passwd name=<身份名>"; exit 1; }
	keepassxc-cli db-edit --set-password identities/$(name)/passwords/$(name).kdbx

pass: ## 复制密码到剪贴板，10 秒后自动清空（name=gene title="Proton 邮箱"）
	@test -n "$(name)" -a -n "$(title)" || { echo '用法: make pass name=gene title="Proton 邮箱"'; exit 1; }
	keepassxc-cli clip identities/$(name)/passwords/$(name).kdbx "$(title)"

pass-show: ## 显示整条记录（name=gene title="..."）
	@test -n "$(name)" -a -n "$(title)" || { echo '用法: make pass-show name=gene title="Proton 邮箱"'; exit 1; }
	keepassxc-cli show identities/$(name)/passwords/$(name).kdbx "$(title)"

pass-add: ## 新增条目并自动生成强密码（name=gene title="..." user="..."）
	@test -n "$(name)" -a -n "$(title)" || { echo '用法: make pass-add name=gene title="Proton 邮箱" user="gene@proton.me"'; exit 1; }
	keepassxc-cli add -g -L 20 -l -U -n -s --every-group --exclude-similar -u "$(user)" identities/$(name)/passwords/$(name).kdbx "$(title)"

pass-add-manual: ## 新增条目，手动输入密码（name=gene title="..." user="..."）
	@test -n "$(name)" -a -n "$(title)" || { echo '用法: make pass-add-manual name=gene title="Proton 邮箱" user="gene@proton.me"'; exit 1; }
	keepassxc-cli add -p -u "$(user)" identities/$(name)/passwords/$(name).kdbx "$(title)"

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

show: db-init ## 打印某身份的全部信息（identity=gene）
	@test -n "$(identity)" || { echo "用法: make show identity=gene"; exit 1; }
	@echo "== $(identity) 的条目 =="
	@sqlite3 -header -column $(DB) "SELECT category AS '分类', key AS '名称', value AS '内容', note AS '备注', updated_at AS '更新时间' FROM items WHERE identity='$(identity)' ORDER BY category, key;"
	@echo
	@echo "== $(identity) 的事件 =="
	@sqlite3 -header -column $(DB) "SELECT happened_at AS '时间', title AS '标题', detail AS '详情' FROM events WHERE identity='$(identity)' ORDER BY happened_at DESC;"

item: db-init ## 写/改条目（identity=gene cat=account key=proton-email value="..." note="..."）
	@test -n "$(identity)" -a -n "$(cat)" -a -n "$(key)" -a -n "$(value)" || { echo '用法: make item identity=gene cat=account key=proton-email value="gene@proton.me" note="主用邮箱"'; exit 1; }
	sqlite3 $(DB) "INSERT INTO items(identity,category,key,value,note) VALUES('$(identity)','$(cat)','$(key)','$(value)','$(note)') ON CONFLICT(identity,category,key) DO UPDATE SET value=excluded.value, note=excluded.note, updated_at=datetime('now');"

event: db-init ## 记事件（identity=gene title="..." detail="..."）
	@test -n "$(identity)" -a -n "$(title)" || { echo '用法: make event identity=gene title="注册 Proton 邮箱" detail="密码在 kdbx"'; exit 1; }
	sqlite3 $(DB) "INSERT INTO events(identity,title,detail) VALUES('$(identity)','$(title)','$(detail)');"

status: db-init ## 总览：身份列表、各身份条目数、Web 运行状态
	@echo "== 身份 =="; ls identities/
	@echo; echo "== 条目数 =="; sqlite3 $(DB) "SELECT identity, COUNT(*) FROM items GROUP BY identity;"
	@echo; echo "== Web =="; docker compose ps 2>/dev/null || true

backup: ## 备份 SQLite 库到 backups/
	@mkdir -p backups
	cp $(DB) backups/id-$$(date +%Y%m%d-%H%M%S).db
	@ls -t backups/ | head -3
