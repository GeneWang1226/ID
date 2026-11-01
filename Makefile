# 参数通过环境变量交给 Rust，避免把资料拼进 shell 命令。
.DEFAULT_GOAL := help
CARGO ?= cargo
BIN := target/debug/id

export ID_ENTRY_ID = $(value id)
export ID_ENTRY_TITLE = $(value title)
ifneq ($(origin user),undefined)
export ID_ENTRY_USER = $(value user)
endif
ifneq ($(origin file),undefined)
export ID_INPUT_FILE = $(value file)
endif
ifneq ($(origin ttl),undefined)
export ID_GRANT_TTL = $(value ttl)
endif
ifneq ($(origin tags),undefined)
export ID_ENTRY_TAGS = $(value tags)
endif
ifneq ($(origin token),undefined)
export ID_GRANT_TOKEN = $(value token)
endif
ifneq ($(origin cmd),undefined)
export ID_GRANT_COMMAND = $(cmd)
endif
ifneq ($(origin cat),undefined)
export ID_ITEM_CATEGORY = $(value cat)
endif
ifneq ($(origin key),undefined)
export ID_ITEM_KEY = $(value key)
endif
ifneq ($(origin value),undefined)
export ID_ITEM_VALUE = $(value value)
endif
ifneq ($(origin note),undefined)
export ID_ITEM_NOTE = $(value note)
endif
ifneq ($(origin title),undefined)
export ID_EVENT_TITLE = $(value title)
endif
ifneq ($(origin detail),undefined)
export ID_EVENT_DETAIL = $(value detail)
endif
manage list: export ID_LIST_TYPE = $(if $(value type),$(value type),all)
manage list: export ID_LIST_CATEGORY = $(value cat)

.PHONY: help build test check settings clipboard-check new passwd pass pass-show manage list pass-list pass-add pass-add-manual pass-edit grant use grants revoke tag show item event ai status backup organize

help: ## 显示帮助
	@awk 'BEGIN {FS = ":.*## "} /^[a-z-]+:.*## / {printf "  make %-17s %s\n", $$1, $$2}' Makefile

$(BIN): Cargo.toml Cargo.lock $(wildcard src/*.rs)
	@$(CARGO) build --locked

build: $(BIN) ## 编译 Rust CLI（源码在 src/）

test: ## 运行 Rust 单元测试和终端集成测试
	@$(CARGO) test --locked
	@$(CARGO) build --locked
	@python3 -m unittest discover -s tests -v

check: $(BIN) ## 检查追踪范围、忽略规则、权限和疑似明文机密
	@$(BIN) check

settings: $(BIN) ## 方向键修改设置、API key 和全局明文权限
	@$(BIN) settings

clipboard-check: $(BIN) ## 重新自测当前 SSH 终端的剪贴板
	@$(BIN) clipboard-check

new: $(BIN) ## 新增或补齐身份（id=gene）
	@$(BIN) new

passwd: $(BIN) ## 修改密码库主密码（id=gene）
	@$(BIN) passwd

pass: $(BIN) ## 复制密码（id/title 可选，缺省选择）
	@$(BIN) pass

pass-show: $(BIN) ## 明文显示密码，须全局允许并重新验证
	@$(BIN) pass-show

manage: $(BIN) ## 身份管理：账号、密码和个人信息（type=all/account/password/info，cat=维度）
	@$(BIN) manage

list: manage

organize: $(BIN) ## 基础信息整理为 ID_info/身份名，旧项目根条目归组（id=gene）
	@$(BIN) organize

pass-list: $(BIN) ## 只查看密码条目
	@$(BIN) pass-list

pass-edit: $(BIN) ## 修改已有密码（id/title 可选）
	@$(BIN) pass-edit

pass-add: $(BIN) ## 新增条目并生成强密码（id/title/user 可选）
	@$(BIN) pass-add

pass-add-manual: $(BIN) ## 选择账号或自定义条目，手动录入密码
	@$(BIN) pass-add-manual

grant: $(BIN) ## 签发临时凭证（id/title 可选，ttl=秒）
	@$(BIN) grant

use: $(BIN) ## 用临时凭证执行（token=... cmd='命令'，变量中的 $ 写成 $$）
	@$(BIN) use

grants: $(BIN) ## 列出活跃临时凭证
	@$(BIN) grants

revoke: $(BIN) ## 撤销凭证（token=... 或 all=1）
	@$(BIN) revoke $(if $(filter 1,$(all)),--all,)

tag: $(BIN) ## 修改标签（id/title 可选，tags=ai-deny；tags= 清空）
	@$(BIN) tag

show: $(BIN) ## 打印身份资料和事件（id 可选）
	@$(BIN) show

item: $(BIN) ## 写入或修改资料（id/cat/key/value/note 可选）
	@$(BIN) item

event: $(BIN) ## 记录事件（id/title/detail 可选）
	@$(BIN) event

ai: $(BIN) ## 全屏 AI 助手：整理资料、录入账号和本地密码操作（id/file 可选）
	@$(BIN) ai

status: $(BIN) ## 查看身份密码库状态
	@$(BIN) status

backup: $(BIN) ## 备份各身份 KDBX 到 backups/vaults/
	@$(BIN) backup
