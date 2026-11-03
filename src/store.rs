use crate::{
    Project, identity,
    kdbx::{self, Document, Node, WriteLock},
    ui,
    vault::Session,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub struct Store<'a> {
    pub project: &'a Project,
    pub identity: &'a str,
    pub session: &'a Session,
}
#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub value: Option<String>,
    pub category: String,
    pub note: String,
}
#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub category: String,
    pub key: String,
    pub value: String,
    pub note: String,
    pub path: String,
    pub icon: u32,
    pub group_icon: u32,
    pub custom_icon: bool,
    pub password_set: bool,
    pub url: String,
    pub fields: Vec<Field>,
    pub attachments: Vec<String>,
    pub public_notes: bool,
    pub email: bool,
}
impl Item {
    pub fn group_path(&self) -> &str {
        self.path
            .rsplit_once('/')
            .map(|(group, _)| group)
            .unwrap_or("根分组")
    }
    pub fn name(&self) -> String {
        self.key.clone()
    }
    pub fn username(&self) -> String {
        self.value.clone()
    }
    pub fn title(&self) -> String {
        self.path.clone()
    }
    pub fn remark(&self) -> String {
        self.note.clone()
    }
    pub fn glyph(&self) -> &'static str {
        icon(self.icon)
    }
}
pub fn icon(id: u32) -> &'static str {
    // KeePass standard IconID order; terminal glyphs do not require a private-use font.
    const GLYPHS: [&str; 69] = [
        "⚿", "◎", "⚠", "▣", "▤", "☏", "▦", "✎", "⇄", "♟", "☷", "▧", "⌁", "⚿", "ϟ", "▱", "★", "◉",
        "▣", "✉", "⚙", "☑", "▤", "▣", "⇆", "▣", "▣", "☁", "♫", "⌘", "⌘", "▥", "◴", "▶", "⚒", "◎",
        "▧", "%", "▦", "◷", "⌕", "⌁", "▥", "♲", "▤", "✕", "?", "▧", "▰", "▱", "▣", "◈", "⚿", "✓",
        "✎", "▧", "☷", "▦", "⚿", "⌘", "⌂", "★", "♟", "✎", "●", "W", "¤", "▣", "▯",
    ];
    GLYPHS.get(id as usize).copied().unwrap_or("◇")
}
pub const ICONS: &[(u32, &str)] = &[
    (0, "密码"),
    (1, "网站"),
    (9, "身份"),
    (22, "文档"),
    (19, "邮箱"),
    (30, "终端"),
    (61, "收藏"),
    (60, "家庭"),
];
pub const EMAIL_FIELDS: &[(&str, bool)] = &[
    ("服务商", false),
    ("地区", false),
    ("用途", false),
    ("应急邮箱", false),
    ("应急手机号", false),
    ("恢复码", true),
    ("恢复短语", true),
    ("授权码", true),
    ("应用密码", true),
    ("IMAP服务器", false),
    ("SMTP服务器", false),
];
pub fn email_address(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\'))
        && !domain.contains('@')
}
pub(crate) fn promote_email_fields(entry: &mut Node) -> bool {
    let aliases = [
        ("原表备注", "用途"),
        ("恢复邮箱", "应急邮箱"),
        ("recovery_email", "应急邮箱"),
        ("恢复手机号", "应急手机号"),
        ("recovery_phone", "应急手机号"),
        ("recovery_codes", "恢复码"),
        ("backup_codes", "恢复码"),
        ("proton_recovery_phrase", "恢复短语"),
        ("recovery_phrase", "恢复短语"),
        ("详细数据 · proton_recovery_phrase", "恢复短语"),
        ("详细数据 · 授权码", "授权码"),
        ("详细数据 · 恢复码", "恢复码"),
        ("详细数据 · 恢复短语", "恢复短语"),
        ("详细数据 · 应急邮箱", "应急邮箱"),
        ("详细数据 · 恢复邮箱", "应急邮箱"),
        ("详细数据 · 应急手机号", "应急手机号"),
        ("app_password", "应用密码"),
        ("详细数据 · 应用密码", "应用密码"),
    ];
    let mut changed = false;
    for (source, target) in aliases {
        if entry.field_value(source).is_empty() || !entry.field_value(target).is_empty() {
            continue;
        }
        let value = zeroize::Zeroizing::new(entry.field_value(source).to_owned());
        let protected = sensitive(target)
            || entry.field(source).is_some_and(kdbx::protected)
            || entry.field(target).is_some_and(kdbx::protected);
        entry.set_field(target, &value, protected);
        changed = true;
    }
    for &(key, secret) in EMAIL_FIELDS {
        if secret
            && let Some(field) = entry.field(key)
            && !kdbx::protected(field)
        {
            let value = zeroize::Zeroizing::new(field.text.to_string());
            entry.set_field(key, &value, true);
            changed = true;
        }
    }
    changed
}
pub fn sensitive(name: &str) -> bool {
    let name = name.to_lowercase().replace([' ', '-', '_'], "");
    [
        "password",
        "passwd",
        "secret",
        "token",
        "apikey",
        "privatekey",
        "recovery",
        "backupcode",
        "otp",
        "seed",
        "mnemonic",
        "密码",
        "密钥",
        "恢复码",
        "恢复短语",
        "助记词",
        "授权码",
        "应急码",
    ]
    .iter()
    .any(|w| name.contains(w))
}
fn public(entry: &Node, key: &str) -> String {
    entry
        .field(key)
        .filter(|n| !kdbx::protected(n))
        .map(|n| n.text.to_string())
        .unwrap_or_default()
}
fn group_roles(doc: &Document) -> BTreeMap<String, String> {
    doc.groups()
        .into_iter()
        .filter(|(_, g)| !g.meta("ID.Group").is_empty())
        .map(|(path, g)| (path, g.meta("ID.Group").to_owned()))
        .collect()
}
fn entry_kind<'a>(entry: &'a Node, path: &str, groups: &BTreeMap<String, String>) -> &'a str {
    match entry.meta("ID.Kind") {
        "info" => "info",
        "event" => "event",
        "account" => "account",
        _ => match path.rsplit_once('/').map(|(parent, _)| parent) {
            Some(parent)
                if groups.get(parent).is_some_and(|k| k == "info") || parent == "ID/身份信息" =>
            {
                "info"
            }
            Some(parent)
                if groups.get(parent).is_some_and(|k| k == "event") || parent == "ID/事件" =>
            {
                "event"
            }
            _ => "account",
        },
    }
}
pub(crate) fn information_id(doc: &Document) -> Result<Option<String>> {
    let groups = group_roles(doc);
    let found = doc
        .entries()
        .into_iter()
        .filter(|(path, e)| entry_kind(e, path, &groups) == "info")
        .map(|(_, e)| e.value("UUID").to_owned())
        .collect::<Vec<_>>();
    ensure!(
        found.len() <= 1,
        "存在多个基础信息条目，请在 KeePassXC 中整理后重试"
    );
    Ok(found.into_iter().next())
}
pub(crate) fn snapshot(doc: &Document) -> Vec<Item> {
    let groups = group_roles(doc);
    let group_icons = doc
        .groups()
        .into_iter()
        .map(|(path, g)| (path, g.value("IconID").parse::<u32>().unwrap_or(48)))
        .collect::<BTreeMap<_, _>>();
    let mut items = doc
        .entries()
        .into_iter()
        .map(|(path, e)| {
            let title = public(e, "Title");
            let kind = entry_kind(e, &path, &groups);
            let mut fields = e
                .children
                .iter()
                .filter(|s| s.name == "String")
                .filter_map(|s| {
                    let key = s.value("Key");
                    if matches!(key, "Title" | "UserName" | "Password" | "URL" | "Notes") {
                        return None;
                    }
                    let value = s.child("Value")?;
                    Some(Field {
                        name: key.into(),
                        value: (!kdbx::protected(value) && !sensitive(key))
                            .then(|| value.text.to_string()),
                        category: e.meta(&format!("ID.Category:{key}")).into(),
                        note: e.meta(&format!("ID.Note:{key}")).into(),
                    })
                })
                .collect::<Vec<_>>();
            let email = e.meta("ID.AccountType") == "email"
                || path.starts_with("ID_emails/")
                || groups
                    .iter()
                    .any(|(group, kind)| kind == "email" && path.starts_with(&format!("{group}/")));
            if email {
                fields.sort_by_key(|field| {
                    EMAIL_FIELDS
                        .iter()
                        .position(|(key, _)| *key == field.name)
                        .unwrap_or(EMAIL_FIELDS.len())
                });
            }
            Item {
                id: e.value("UUID").into(),
                category: kind.into(),
                key: title,
                value: public(e, "UserName"),
                note: public(e, "Notes"),
                icon: e.value("IconID").parse().unwrap_or(0),
                group_icon: group_icons
                    .get(
                        path.rsplit_once('/')
                            .map(|(parent, _)| parent)
                            .unwrap_or(""),
                    )
                    .copied()
                    .unwrap_or(48),
                path,
                custom_icon: !e.value("CustomIconUUID").is_empty()
                    && e.value("CustomIconUUID") != "AAAAAAAAAAAAAAAAAAAAAA==",
                password_set: !e.field_value("Password").is_empty(),
                url: public(e, "URL"),
                fields,
                attachments: e
                    .children
                    .iter()
                    .filter(|n| n.name == "Binary")
                    .map(|n| n.value("Key").to_owned())
                    .collect(),
                public_notes: e.meta("ID.PublicNotes") == "true",
                email,
            }
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| (&a.category, &a.key).cmp(&(&b.category, &b.key)));
    items
}
pub(crate) fn put_document(
    doc: &mut Document,
    category: &str,
    key: &str,
    value: &str,
    note: &str,
) -> Result<()> {
    ensure!(
        !key.trim().is_empty() && !value.trim().is_empty(),
        "名称和内容不能为空"
    );
    let email = category == "email";
    let category = item_category(category, key);
    ensure!(
        matches!(category, "account" | "email") || !sensitive(key),
        "机密字段请在本地管理界面中录入"
    );
    if matches!(category, "account" | "email") {
        ensure!(!email || email_address(value), "邮箱地址无效");
        let id = match doc.find_entry(key)? {
            Some(id) => id,
            None => doc.add(
                key,
                if email { "email" } else { "account" },
                if value.contains('@') { 19 } else { 1 },
            )?,
        };
        let entry = doc.entry_mut(&id)?;
        ensure!(
            !matches!(entry.meta("ID.Kind"), "info" | "event")
                && entry.field_value("Title") != "基础信息",
            "同名基础信息条目不能覆盖为账号"
        );
        ensure!(
            entry.field("UserName").is_none_or(|n| !kdbx::protected(n)),
            "受保护登录名请在本地编辑"
        );
        ensure!(
            entry.field("Notes").is_none_or(|n| !kdbx::protected(n)),
            "受保护备注请在 KeePassXC 中编辑"
        );
        entry.set_field("UserName", value, false);
        entry.set_field("Notes", note, false);
        entry.set_meta("ID.PublicNotes", "true");
        entry.touch();
        if email {
            doc.email(&id)?;
        }
    } else {
        let id = match information_id(doc)? {
            Some(id) => id,
            None => doc.add("基础信息", "info", 9)?,
        };
        let entry = doc.entry_mut(&id)?;
        ensure!(
            !matches!(key, "Title" | "UserName" | "Password" | "URL" | "Notes"),
            "该名称为 KeePass 标准字段，请更换名称"
        );
        ensure!(
            entry.field(key).is_none_or(|n| !kdbx::protected(n)),
            "已有受保护字段，未覆盖"
        );
        entry.set_field(key, value, false);
        entry.set_meta(&format!("ID.Category:{key}"), category);
        entry.set_meta(&format!("ID.Note:{key}"), note);
        entry.touch();
    }
    Ok(())
}
impl<'a> Store<'a> {
    pub fn open(project: &'a Project, identity: &'a str, session: &'a Session) -> Result<Self> {
        identity::vault(project, identity)?;
        Ok(Self {
            project,
            identity,
            session,
        })
    }
    fn scope(&self, identity: &str) -> Result<()> {
        ensure!(identity == self.identity, "不能访问其他身份");
        Ok(())
    }
    pub fn items(&self, identity: &str) -> Result<Vec<Item>> {
        self.scope(identity)?;
        Ok(snapshot(&Document::load(
            self.project,
            identity,
            self.session,
        )?))
    }
    pub(crate) fn update<T>(&self, edit: impl FnOnce(&mut Document) -> Result<T>) -> Result<T> {
        let _lock = WriteLock::acquire(self.project, self.identity)?;
        let mut doc = Document::load(self.project, self.identity, self.session)?;
        let result = edit(&mut doc)?;
        doc.save(self.project, self.identity, self.session)?;
        Ok(result)
    }
    pub fn put(
        &self,
        identity: &str,
        category: &str,
        key: &str,
        value: &str,
        note: &str,
    ) -> Result<()> {
        self.scope(identity)?;
        self.update(|d| put_document(d, category, key, value, note))
    }
    pub fn create_account(&self, title: &str, user: &str, note: &str) -> Result<()> {
        self.update(|d| {
            ensure!(d.find_entry(title)?.is_none(), "已有同名条目，未覆盖");
            put_document(d, "account", title, user, note)
        })
    }
    pub fn create_email(&self, title: &str, user: &str, note: &str) -> Result<()> {
        self.update(|d| {
            ensure!(d.find_entry(title)?.is_none(), "已有同名条目，未覆盖");
            put_document(d, "email", title, user, note)
        })
    }
    pub fn email(&self, id: &str) -> Result<()> {
        self.update(|d| d.email(id).map(|_| ()))
    }
    pub fn event(&self, identity: &str, title: &str, detail: &str) -> Result<()> {
        self.scope(identity)?;
        self.batch(&[], &[(title.into(), detail.into())])
    }
    pub fn organize(&self) -> Result<()> {
        let _lock = WriteLock::acquire(self.project, self.identity)?;
        let mut doc = Document::load(self.project, self.identity, self.session)?;
        let count = doc.organize(self.identity)?;
        if count == 0 {
            println!("项目条目已归组，无需整理。");
            return Ok(());
        }
        if !ui::confirm(&format!(
            "整理 {count} 项项目资料；基础信息统一到 ID_info/{}，邮箱归入 ID_emails，旧项目根条目归组。先留加密备份，保留 UUID、密码、附件和历史。",
            self.identity
        ))? {
            return Ok(());
        }
        doc.save(self.project, self.identity, self.session)?;
        println!(
            "已整理 {count} 项项目资料，基础信息位于 ID_info/{}。",
            self.identity
        );
        Ok(())
    }
    pub fn batch(
        &self,
        items: &[(String, String, String, String)],
        events: &[(String, String)],
    ) -> Result<()> {
        self.update(|d| {
            for (cat, key, value, note) in items {
                put_document(d, cat, key, value, note)?;
            }
            for (title, detail) in events {
                ensure!(!title.trim().is_empty(), "事件标题不能为空");
                let id = d.add(title, "event", 22)?;
                let e = d.entry_mut(&id)?;
                e.set_field("Notes", detail, false);
                e.set_meta("ID.PublicNotes", "true");
                e.touch();
            }
            Ok(())
        })
    }
    pub fn note(&self, identity: &str, id: &str, text: &str) -> Result<()> {
        self.scope(identity)?;
        self.update(|d| {
            let e = d.entry_mut(id)?;
            ensure!(
                e.field("Notes").is_none_or(|n| !kdbx::protected(n)),
                "受保护备注请在 KeePassXC 中编辑"
            );
            e.set_field("Notes", text, false);
            e.set_meta("ID.PublicNotes", "true");
            e.touch();
            Ok(())
        })
    }
    pub fn field(
        &self,
        id: &str,
        key: &str,
        value: &str,
        secret: bool,
        category: &str,
        note: &str,
    ) -> Result<()> {
        ensure!(
            !key.trim().is_empty() && !key.chars().any(char::is_control),
            "字段名无效"
        );
        ensure!(
            !matches!(key, "Title" | "Password" | "Notes"),
            "此标准字段请用对应操作编辑"
        );
        self.update(|d| {
            let e = d.entry_mut(id)?;
            ensure!(
                secret || e.field(key).is_none_or(|n| !kdbx::protected(n)),
                "不能将受保护字段改为普通文字"
            );
            e.set_field(key, value, secret || sensitive(key));
            e.set_meta(&format!("ID.Category:{key}"), category);
            e.set_meta(&format!("ID.Note:{key}"), note);
            e.touch();
            Ok(())
        })
    }
    pub fn icon(&self, id: &str, icon: u32) -> Result<()> {
        ensure!(icon < 69, "图标编号应为 0–68");
        self.update(|d| {
            let e = d.entry_mut(id)?;
            e.set("IconID", &icon.to_string());
            e.children.retain(|n| n.name != "CustomIconUUID");
            e.touch();
            Ok(())
        })
    }
    pub fn delete_field(&self, identity: &str, id: &str, key: &str) -> Result<()> {
        self.scope(identity)?;
        ensure!(!matches!(key, "Title" | "Password"), "此字段不能直接删除");
        self.session
            .unlock(&identity::vault(self.project, identity)?, true)?;
        self.update(|d| {
            let e = d.entry_mut(id)?;
            ensure!(e.field(key).is_some(), "字段不存在");
            e.remove_field(key);
            if key == "Notes" {
                e.set_meta("ID.PublicNotes", "");
            }
            e.set_meta(&format!("ID.Category:{key}"), "");
            e.set_meta(&format!("ID.Note:{key}"), "");
            e.touch();
            Ok(())
        })
    }
    pub fn delete(&self, identity: &str, id: &str) -> Result<()> {
        self.scope(identity)?;
        self.session
            .unlock(&identity::vault(self.project, identity)?, true)?;
        self.update(|d| {
            d.entry_mut(id)?;
            let path = d
                .entries()
                .into_iter()
                .find(|(_, e)| e.value("UUID") == id)
                .context("条目不存在")?
                .0;
            ensure!(
                d.find_path(&path)?.as_deref() == Some(id),
                "条目路径有歧义，未删除"
            );
            d.deletions.push(path);
            fn remove(n: &mut Node, id: &str) {
                n.children
                    .retain(|c| c.name != "Entry" || c.value("UUID") != id);
                for c in &mut n.children {
                    if c.name != "History" {
                        remove(c, id);
                    }
                }
            }
            remove(&mut d.root, id);
            Ok(())
        })
    }
}
pub fn password_states(
    project: &Project,
    identity: &str,
    session: &Session,
) -> Result<BTreeMap<String, String>> {
    Ok(snapshot(&Document::load(project, identity, session)?)
        .into_iter()
        .filter(|i| i.category == "account")
        .map(|i| {
            (
                i.title(),
                if i.password_set {
                    "已设置"
                } else {
                    "未填密码"
                }
                .into(),
            )
        })
        .collect())
}
pub fn resolve_path(
    project: &Project,
    identity: &str,
    title: &str,
    session: &Session,
) -> Result<Option<String>> {
    let doc = Document::load(project, identity, session)?;
    let Some(id) = doc.find_entry(title)? else {
        return Ok(None);
    };
    let path = doc
        .entries()
        .into_iter()
        .find(|(_, e)| e.value("UUID") == id)
        .context("条目不存在")?
        .0;
    ensure!(
        doc.find_path(&path)?.as_deref() == Some(id.as_str()),
        "条目路径有歧义，请在 KeePassXC 中整理"
    );
    Ok(Some(path))
}
pub fn write_password(
    project: &Project,
    identity: &str,
    title: Option<&str>,
    username: Option<&str>,
    edit: bool,
    generate: bool,
    session: &Session,
) -> Result<String> {
    ui::require_terminal()?;
    let vault = identity::vault(project, identity)?;
    let mut title = match title.filter(|t| !t.is_empty()) {
        Some(t) => t.into(),
        None if edit => select_password(project, identity, "", session)?.context("已取消")?,
        None => ui::input("条目标题: ")?,
    };
    ensure!(
        !title.trim().is_empty() && !title.chars().any(char::is_control),
        "条目标题无效"
    );
    session.unlock(&vault, edit)?;
    let mut doc = Document::load(project, identity, session)?;
    let existing = doc.find_entry(&title)?;
    let mut replace = false;
    if let Some(id) = &existing {
        let e = doc
            .entries()
            .into_iter()
            .find(|(_, e)| e.value("UUID") == id)
            .unwrap()
            .1;
        ensure!(
            e.meta("ID.Kind") != "info" && e.field_value("Title") != "基础信息",
            "基础信息不使用账号密码操作"
        );
        ensure!(e.meta("ID.Kind") != "event", "事件不使用账号密码操作");
        replace = !e.field_value("Password").is_empty();
        title = doc
            .entries()
            .into_iter()
            .find(|(_, n)| n.value("UUID") == id)
            .unwrap()
            .0;
    }
    ensure!(!edit || existing.is_some(), "条目不存在");
    if replace && !edit {
        session.unlock(&vault, true)?;
    }
    let user = match username {
        Some(s) => Some(s.to_owned()),
        None if existing.is_none() => Some(ui::input("用户名（可空）: ")?),
        None => None,
    };
    let password = if generate {
        const CHARS: &[u8] =
            b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#$%^&*-_+";
        let mut secret = crate::vault::Secret::empty();
        for _ in 0..32 {
            secret.push(&[CHARS[rand::random_range(0..CHARS.len())]])?;
        }
        secret
    } else {
        ui::secret("新条目密码: ")?
    };
    let _lock = WriteLock::acquire(project, identity)?;
    let id = match existing {
        Some(id) => id,
        None => doc.add(&title, "account", 0)?,
    };
    let entry = doc.entry_mut(&id)?;
    if let Some(user) = user {
        entry.set_field("UserName", &user, false);
    }
    entry.set_field("Password", std::str::from_utf8(password.bytes())?, true);
    entry.touch();
    title = doc
        .entries()
        .into_iter()
        .find(|(_, e)| e.value("UUID") == id)
        .context("条目不存在")?
        .0;
    doc.save(project, identity, session)?;
    println!("密码已保存。");
    Ok(title)
}

pub fn category_label(category: &str) -> &str {
    match category {
        "address" => "地址",
        "contact" => "联系方式",
        "preference" => "偏好",
        "id-info" => "身份信息",
        "work" => "工作",
        "education" => "教育",
        "health" => "健康",
        "finance" => "财务",
        "asset" => "资产",
        _ => category,
    }
}

pub fn item_category<'a>(category: &'a str, key: &str) -> &'a str {
    let key = key.trim().to_lowercase().replace([' ', '-', '_'], "");
    if category == "account"
        && matches!(
            key.as_str(),
            "电话号码" | "手机号" | "手机号码" | "联系电话" | "phone" | "phonenumber" | "telephone"
        )
    {
        "contact"
    } else {
        category
    }
}
pub fn infer_title(text: &str) -> String {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let re = regex::Regex::new(r"([\p{Han}A-Za-z0-9_.-]+?)(?:账号|账户)?密码").unwrap();
    let Some(m) = re.captures(&compact) else {
        return text.trim().to_owned();
    };
    let mut p = m[1].to_owned();
    loop {
        let before = p.clone();
        for word in [
            "重新设置",
            "查一下",
            "给我的",
            "查看",
            "查询",
            "看看",
            "复制",
            "显示",
            "保存",
            "设置",
            "重置",
            "修改",
            "帮我",
            "请",
            "我的",
            "给我",
            "给",
            "看",
        ] {
            if let Some(rest) = p.strip_prefix(word) {
                p = rest.into();
                break;
            }
        }
        if p == before {
            break;
        }
    }
    for suffix in [
        "账号保存",
        "账户保存",
        "账号",
        "账户",
        "保存",
        "设置",
        "重置",
        "修改",
    ] {
        if let Some(rest) = p.strip_suffix(suffix) {
            p = rest.into();
            break;
        }
    }
    if p.is_empty() {
        "密码".into()
    } else {
        format!("{p}密码")
    }
}
fn match_key(text: &str) -> String {
    let mut key: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    for suffix in [
        "账户密码",
        "账号密码",
        "密码",
        "password",
        "passwd",
        "passcode",
    ] {
        if let Some(rest) = key.strip_suffix(suffix) {
            key = rest.into();
            break;
        }
    }
    key
}
pub fn select_password(
    project: &Project,
    identity: &str,
    query: &str,
    session: &Session,
) -> Result<Option<String>> {
    let states = password_states(project, identity, session)?;
    let term = match_key(&infer_title(query));
    let mut paths: Vec<String> = states
        .keys()
        .filter(|p| term.is_empty() || match_key(p).contains(&term))
        .cloned()
        .collect();
    if paths.is_empty() {
        paths = states.keys().cloned().collect();
    }
    let choices = paths
        .iter()
        .map(|p| format!("{} · {}", p, states[p]))
        .collect::<Vec<_>>();
    Ok(ui::choose("选择密码条目", &choices)?.map(|i| paths[i].clone()))
}

#[derive(Clone)]
pub struct Record {
    pub item: Item,
    pub title: String,
    pub status: String,
}
impl Record {
    pub fn kind(&self) -> &str {
        &self.item.category
    }
    pub fn group(&self) -> String {
        format!(
            "{} {} · {}",
            icon(self.item.group_icon),
            self.item.group_path(),
            match self.kind() {
                "info" => "基础信息",
                "event" => "事件",
                _ if self.item.email => "邮箱与密码",
                _ => "账号与密码",
            }
        )
    }
    pub fn details(&self, _items: &[Item]) -> Vec<(String, String)> {
        let i = &self.item;
        let mut fields = vec![
            ("条目".into(), i.key.clone()),
            ("群组".into(), i.group_path().into()),
        ];
        if i.category == "account" {
            fields.extend([
                ("登录名".into(), i.value.clone()),
                ("密码".into(), self.status.clone()),
                ("网址".into(), i.url.clone()),
            ]);
        }
        if !i.note.is_empty() {
            fields.push(("备注".into(), i.note.clone()));
        }
        for f in &i.fields {
            let label = if f.category.is_empty() {
                f.name.clone()
            } else {
                format!("{} / {}", category_label(&f.category), f.name)
            };
            fields.push((
                label,
                f.value.clone().unwrap_or_else(|| "••••（受保护）".into()),
            ));
            if !f.note.is_empty() {
                fields.push((format!("{} · 备注", f.name), f.note.clone()));
            }
        }
        if !i.attachments.is_empty() {
            fields.push(("附件".into(), i.attachments.join("\n")));
        }
        fields.push((
            "图标".into(),
            format!(
                "{}  #{}{}",
                i.glyph(),
                i.icon,
                if i.custom_icon {
                    " · 自定义图标"
                } else {
                    ""
                }
            ),
        ));
        fields
    }
}
pub fn records(
    items: &[Item],
    _states: &BTreeMap<String, String>,
    view: &str,
    category: &str,
) -> Vec<Record> {
    let mut records = items
        .iter()
        .filter(|i| match view {
            "account" => i.category == "account",
            "password" => i.category == "account" && i.password_set,
            "info" => i.category == "info",
            _ => true,
        })
        .filter(|i| {
            category.is_empty()
                || i.category != "info"
                || i.fields.iter().any(|f| f.category == category)
        })
        .map(|i| {
            let mut item = i.clone();
            if !category.is_empty() && item.category == "info" {
                item.fields.retain(|f| f.category == category);
            }
            Record {
                title: i.title(),
                status: if i.category == "account" {
                    if i.password_set {
                        "已设置"
                    } else {
                        "未填密码"
                    }
                } else {
                    ""
                }
                .into(),
                item,
            }
        })
        .collect::<Vec<_>>();
    records.sort_by(|a, b| {
        (&a.item.category, a.item.group_path(), &a.item.key).cmp(&(
            &b.item.category,
            b.item.group_path(),
            &b.item.key,
        ))
    });
    records
}

pub use crate::kdbx::backup;
pub fn write_lock(project: &Project, identity: &str) -> Result<impl Sized> {
    WriteLock::acquire(project, identity)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn email_group_presets_preserve_secrets_and_do_not_classify_login_addresses() {
        let mut doc = Document {
            root: kdbx::parse(b"<KeePassFile><Root><Group/></Root></KeePassFile>").unwrap(),
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        put_document(&mut doc, "account", "GitHub", "tester@example.com", "").unwrap();
        put_document(&mut doc, "email", "Proton", "tester@example.com", "Mailbox").unwrap();
        assert!(doc.find_path("ID/账号/GitHub").unwrap().is_some());
        let id = doc.find_path("ID_emails/Proton").unwrap().unwrap();
        let e = doc.entry_mut(&id).unwrap();
        e.set_field("proton_recovery_phrase", "001234", true);
        e.set_field("backup_codes", "old-code", true);
        e.set_field("恢复码", "current-code", false);
        e.set_field("恢复邮箱", "recovery@example.com", true);
        assert!(promote_email_fields(e));
        assert_eq!(e.field_value("恢复短语"), "001234");
        assert_eq!(e.field_value("proton_recovery_phrase"), "001234");
        assert_eq!(e.field_value("恢复码"), "current-code");
        assert_eq!(e.field_value("backup_codes"), "old-code");
        assert!(kdbx::protected(e.field("恢复码").unwrap()));
        assert!(kdbx::protected(e.field("应急邮箱").unwrap()));
        assert!(!promote_email_fields(e));
        let items = snapshot(&doc);
        assert!(!items.iter().find(|i| i.key == "GitHub").unwrap().email);
        let mail = items.iter().find(|i| i.key == "Proton").unwrap();
        assert!(mail.email);
        assert!(mail.fields.iter().all(|f| f.value.is_none()));
        assert!(put_document(&mut doc, "email", "Invalid", "not-an-email", "").is_err());
        for address in ["x@example.com", "x+tag@example.com"] {
            assert!(email_address(address));
        }
        for address in [
            "x",
            "@example.com",
            "x@example",
            "x@@example.com",
            "x @example.com",
            "x@.example.com",
        ] {
            assert!(!email_address(address));
        }
    }
    #[test]
    fn native_snapshot_never_exposes_protected_or_secret_named_fields() {
        let xml=br#"<KeePassFile><Root><Group><Entry><UUID>a</UUID><IconID>20</IconID><String><Key>Title</Key><Value>Mail</Value></String><String><Key>Password</Key><Value>synthetic-password</Value></String><String><Key>api_key</Key><Value>synthetic-key</Value></String><String><Key>recovery</Key><Value ProtectInMemory="True">synthetic-code</Value></String><String><Key>region</Key><Value>Earth</Value></String></Entry></Group></Root></KeePassFile>"#;
        let d = Document {
            root: kdbx::parse(xml).unwrap(),
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        let i = &snapshot(&d)[0];
        assert!(i.password_set);
        assert_eq!(i.icon, 20);
        assert_eq!(i.fields.iter().filter(|f| f.value.is_some()).count(), 1);
        assert_eq!(i.fields[2].value.as_deref(), Some("Earth"));
    }
    #[test]
    fn info_is_one_native_entry_with_fields_and_account_stays_native() {
        let mut d = Document {
            root: kdbx::parse(b"<KeePassFile><Root><Group/></Root></KeePassFile>").unwrap(),
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        put_document(&mut d, "id-info", "姓名", "Synthetic", "备注").unwrap();
        put_document(&mut d, "account", "电话号码", "1234", "").unwrap();
        put_document(&mut d, "account", "GitHub", "user", "").unwrap();
        put_document(&mut d, "account", "GitHub", "new-user", "").unwrap();
        let items = snapshot(&d);
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].fields.len(), 2);
        assert_eq!(items[0].value, "new-user");
        assert!(put_document(&mut d, "info", "api key", "secret", "").is_err());
        assert_eq!(infer_title("查一下我的github密码"), "github密码");
        put_document(&mut d, "account", "1Password", "public-login", "").unwrap();
        let info = d.find_entry("基础信息").unwrap().unwrap();
        d.entry_mut(&info)
            .unwrap()
            .set_field("隐藏属性", "original", true);
        assert!(put_document(&mut d, "id-info", "隐藏属性", "replacement", "").is_err());
        assert_eq!(
            d.entry_mut(&info).unwrap().field_value("隐藏属性"),
            "original"
        );
    }
    #[test]
    fn information_namespace_does_not_claim_another_groups_entry() {
        let mut doc=Document{root:kdbx::parse("<KeePassFile><Root><Group><UUID>root</UUID><Group><UUID>work</UUID><Name>Work</Name><Entry><UUID>external</UUID><String><Key>Title</Key><Value>基础信息</Value></String><String><Key>UserName</Key><Value>original</Value></String></Entry></Group></Group></Root></KeePassFile>".as_bytes()).unwrap(),fingerprint:[0;32],deletions:Vec::new(),original_paths:Default::default()};
        put_document(&mut doc, "id-info", "姓名", "Synthetic", "").unwrap();
        let information = information_id(&doc).unwrap().unwrap();
        assert_ne!(information, "external");
        assert!(doc.find_path("ID/身份信息/基础信息").unwrap().is_some());
        let outside = doc.entry_mut("external").unwrap();
        assert_eq!(outside.field_value("UserName"), "original");
        assert!(outside.field("姓名").is_none());
        assert_eq!(
            snapshot(&doc)
                .iter()
                .find(|i| i.id == "external")
                .unwrap()
                .category,
            "account"
        );
    }
}
