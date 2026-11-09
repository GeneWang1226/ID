# 参数通过环境变量交给 Rust，避免把资料拼进 shell 命令。
.DEFAULT_GOAL := help
CARGO ?= $(shell command -v cargo 2>/dev/null || { \
	cargo_path="$${CARGO_HOME:-$$HOME/.cargo}/bin/cargo"; \
	if [ -x "$$cargo_path" ]; then printf '%s' "$$cargo_path"; else printf '%s' cargo; fi; })
BIN := target/debug/id

define require_cargo
	@command -v "$(CARGO)" >/dev/null 2>&1 || { \
		printf '%s\n' '未找到 Cargo，需要安装 Rust 1.89+。' \
		'安装：https://rust-lang.org/tools/install/' \
		'已安装？加载环境：. "$$HOME/.cargo/env"，或用 make CARGO=/路径/cargo 指定。'; \
		exit 1; \
	}
endef

export ID_ENTRY_ID = $(value id)
export ID_ENTRY_TITLE = $(value title)
ifneq ($(origin field),undefined)
export ID_ENTRY_FIELD = $(value field)
endif
ifneq ($(origin show),undefined)
export ID_PASS_SHOW = $(value show)
endif
ifneq ($(origin generate),undefined)
export ID_PASS_GENERATE = $(value generate)
endif
ifneq ($(origin fix),undefined)
export ID_DOCTOR_FIX = $(value fix)
endif
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
manage: export ID_LIST_TYPE = $(if $(value type),$(value type),all)
manage: export ID_LIST_CATEGORY = $(value cat)

.PHONY: help build test check settings clipboard-check new remove passwd pass pass-set manage grant use grants revoke tag item event ai status backup organize doctor

help:
	@awk 'BEGIN {FS = ":.*## "; print "使用：make <命令> [id=身份]"} /^##@ / {printf "\n%s\n", substr($$0, 5)} /^[a-z-]+:.*## / {printf "  %-18s %s\n", $$1, $$2}' Makefile

$(BIN): Cargo.toml Cargo.lock $(wildcard src/*.rs)
	$(require_cargo)
	@"$(CARGO)" build --locked

##@ 日常使用

manage: $(BIN) ## 管理账号、密码、资料与附件；type=all/account/password/info，cat=维度
	@$(BIN) manage

ai: $(BIN) ## 全屏 AI 助手；file=非机密资料文件
	@$(BIN) ai

pass: $(BIN) ## 复制密码或 field=字段名；title=条目，show=1 明文查看（须设置允许）
	@$(BIN) pass

pass-set: $(BIN) ## 录入密码或 field=受保护字段；title=已有条目，generate=1 生成密码
	@$(BIN) pass-set

settings: $(BIN) ## 方向键修改设置、API key 和全局明文权限
	@$(BIN) settings

##@ 身份管理

new: $(BIN) ## 新建独立身份密码库（id=gene）
	@$(BIN) new

remove: $(BIN) ## 移除指定身份；确认、验证后归档原文件，不直接销毁
	@$(BIN) remove

passwd: $(BIN) ## 修改密码库主密码（id=gene）
	@$(BIN) passwd

status: $(BIN) ## 查看所有身份密码库状态
	@$(BIN) status

##@ 维护

doctor: $(BIN) ## 检查 identities 异常；id=身份，fix=1 确认后归档加密副本
	@$(BIN) doctor

backup: $(BIN) ## 备份各身份加密密码库到 backups/vaults/
	@$(BIN) backup

organize: $(BIN) ## 整理 ID_info/身份名、ID_emails 与旧项目根条目（id=gene）
	@$(BIN) organize

clipboard-check: $(BIN) ## 重新自测当前 SSH 终端的剪贴板
	@$(BIN) clipboard-check

##@ 自动化与授权

item: $(BIN) ## 写入普通资料；cat=维度/account/email key=名称 value=内容 note=备注
	@$(BIN) item

event: $(BIN) ## 记录事件；title=标题 detail=详情
	@$(BIN) event

tag: $(BIN) ## 修改条目标签；title=条目 tags=ai-deny（tags= 清空）
	@$(BIN) tag

grant: $(BIN) ## 签发临时凭证（id/title 可选，ttl=秒）
	@$(BIN) grant

use: $(BIN) ## 用临时凭证执行（token=... cmd='命令'，变量中的 $ 写成 $$）
	@$(BIN) use

grants: $(BIN) ## 列出活跃临时凭证
	@$(BIN) grants

revoke: $(BIN) ## 撤销凭证（token=... 或 all=1）
	@$(BIN) revoke $(if $(filter 1,$(all)),--all,)

##@ 开发

build: $(BIN) ## 编译 Rust CLI

test: ## 运行单元测试和终端集成测试
	$(require_cargo)
	@"$(CARGO)" test --locked
	@"$(CARGO)" build --locked
	@python3 -m unittest discover -s tests -v

check: $(BIN) ## 检查追踪范围、忽略规则、权限和疑似机密
	@$(BIN) check
