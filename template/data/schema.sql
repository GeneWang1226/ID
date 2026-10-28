-- SQLite 个人信息表结构
-- 定位：存放「非机密」的个人信息（账号名、地址、偏好、联系方式等）。
-- 铁律：密码、密钥、恢复码等机密永远放 passwords/ 里的 .kdbx，不进此库。

-- 通用键值表：大多数个人信息直接放这里
CREATE TABLE IF NOT EXISTS items (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    identity   TEXT NOT NULL,               -- 身份名，如 gene / jacob
    category   TEXT NOT NULL,               -- 分类，如 account / address / preference / id-info / contact
    key        TEXT NOT NULL,               -- 条目名，如 github-username
    value      TEXT NOT NULL,               -- 内容
    note       TEXT NOT NULL DEFAULT '',    -- 备注
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (identity, category, key)
);

-- 时间线表：记录某身份的重要事件（注册、换绑、认证等）
CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    identity    TEXT NOT NULL,              -- 身份名
    happened_at TEXT NOT NULL DEFAULT (datetime('now')),
    title       TEXT NOT NULL,
    detail      TEXT NOT NULL DEFAULT ''
);

-- 常用查询示例：
--   查 gene 的全部账号:  SELECT key, value FROM items WHERE identity='gene' AND category='account';
--   写入/更新:          INSERT INTO items(identity,category,key,value) VALUES('gene','account','github','myname')
--                       ON CONFLICT(identity,category,key) DO UPDATE SET value=excluded.value, updated_at=datetime('now');
--   记事件:             INSERT INTO events(identity,title,detail) VALUES('gene','注册 GitHub','用主邮箱');
