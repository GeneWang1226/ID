# ID 项目常用操作。用法: make <目标> [变量=值]
# 例: make new name=gene
#     make item identity=gene cat=account key=proton-email value="gene@proton.me"

DB := data/id.db

.PHONY: help new passwd web web-stop web-logs db item event status backup

help: ## 显示本帮助
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  make %-10s %s\n", $$1, $$2}'

new: ## 新增身份（name=gene）
	@test -n "$(NAME)" || { echo "用法: make new name=<身份名>"; exit 1; }
	scripts/add-identity.sh $(NAME)

passwd: ## 修改某身份 kdbx 主密码（name=gene）
	@test -n "$(NAME)" || { echo "用法: make passwd name=<身份名>"; exit 1; }
	keepassxc-cli db-edit --set-password identities/$(NAME)/passwords/$(NAME).kdbx

pass: ## 复制密码到剪贴板，10 秒后自动清空（name=gene title="Proton 邮箱"）
	@test -n "$(NAME)" -a -n "$(TITLE)" || { echo '用法: make pass name=gene title="Proton 邮箱"'; exit 1; }
	keepassxc-cli clip identities/$(NAME)/passwords/$(NAME).kdbx "$(TITLE)"

pass-show: ## 显示整条记录（name=gene title="..."）
	@test -n "$(NAME)" -a -n "$(TITLE)" || { echo '用法: make pass-show name=gene title="Proton 邮箱"'; exit 1; }
	keepassxc-cli show identities/$(NAME)/passwords/$(NAME).kdbx "$(TITLE)"

pass-add: ## 新增条目并自动生成强密码（name=gene title="..." user="..."）
	@test -n "$(NAME)" -a -n "$(TITLE)" || { echo '用法: make pass-add name=gene title="Proton 邮箱" user="gene@proton.me"'; exit 1; }
	keepassxc-cli add -g -L 20 -l -U -n -s --every-group --exclude-similar -u "$(USER)" identities/$(NAME)/passwords/$(NAME).kdbx "$(TITLE)"

pass-add-manual: ## 新增条目，手动输入密码（name=gene title="..." user="..."）
	@test -n "$(NAME)" -a -n "$(TITLE)" || { echo '用法: make pass-add-manual name=gene title="Proton 邮箱" user="gene@proton.me"'; exit 1; }
	keepassxc-cli add -p -u "$(USER)" identities/$(NAME)/passwords/$(NAME).kdbx "$(TITLE)"

web: ## 构建并启动 Web 界面（http://localhost:3000）
	docker compose up -d --build

web-stop: ## 停止 Web 界面
	docker compose down

web-logs: ## 查看 Web 日志
	docker compose logs -f web

db: ## 打开 SQLite 交互式查询
	sqlite3 $(DB)

item: ## 写/改条目（identity=gene cat=account key=proton-email value="..." note="..."）
	@test -n "$(IDENTITY)" -a -n "$(CAT)" -a -n "$(KEY)" -a -n "$(VALUE)" || { echo '用法: make item identity=gene cat=account key=proton-email value="gene@proton.me" note="主用邮箱"'; exit 1; }
	sqlite3 $(DB) "INSERT INTO items(identity,category,key,value,note) VALUES('$(IDENTITY)','$(CAT)','$(KEY)','$(VALUE)','$(NOTE)') ON CONFLICT(identity,category,key) DO UPDATE SET value=excluded.value, note=excluded.note, updated_at=datetime('now');"

event: ## 记事件（identity=gene title="..." detail="..."）
	@test -n "$(IDENTITY)" -a -n "$(TITLE)" || { echo '用法: make event identity=gene title="注册 Proton 邮箱" detail="密码在 kdbx"'; exit 1; }
	sqlite3 $(DB) "INSERT INTO events(identity,title,detail) VALUES('$(IDENTITY)','$(TITLE)','$(DETAIL)');"

status: ## 总览：身份列表、各身份条目数、Web 运行状态
	@echo "== 身份 =="; ls identities/
	@echo; echo "== 条目数 =="; sqlite3 $(DB) "SELECT identity, COUNT(*) FROM items GROUP BY identity;" 2>/dev/null || echo "（库还没建）"
	@echo; echo "== Web =="; docker compose ps 2>/dev/null || true

backup: ## 备份 SQLite 库到 backups/
	@mkdir -p backups
	cp $(DB) backups/id-$$(date +%Y%m%d-%H%M%S).db
	@ls -t backups/ | head -3
