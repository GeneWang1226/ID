use crate::{Project, identity, private_dir, private_file, ui, vault::Session};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
};

pub struct Store {
    pub db: Connection,
}
#[derive(Clone, Debug)]
pub struct Item {
    pub id: i64,
    pub category: String,
    pub key: String,
    pub value: String,
    pub note: String,
    pub link: Option<String>,
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
const INTERNAL: &[&str] = &[
    "type",
    "label",
    "password_title",
    "source",
    "source_row",
    "source_owner",
    "username",
];
pub fn fields(note: &str) -> Vec<(String, String, String)> {
    match serde_json::from_str::<Value>(note) {
        Ok(Value::Object(values)) => values
            .into_iter()
            .filter(|(k, v)| !INTERNAL.contains(&k.as_str()) && !v.is_null() && v != "")
            .map(|(k, v)| {
                let label = match k.as_str() {
                    "provider" => "服务商",
                    "region" => "地区",
                    "purpose" | "text" => "备注",
                    _ => &k,
                }
                .to_owned();
                (
                    format!("meta:{k}"),
                    label,
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string()),
                )
            })
            .collect(),
        _ if !note.is_empty() => vec![("note".into(), "备注".into(), note.into())],
        _ => Vec::new(),
    }
}
impl Item {
    pub fn metadata(&self) -> Value {
        serde_json::from_str(&self.note).unwrap_or(Value::Null)
    }
    pub fn name(&self) -> String {
        let meta = self.metadata();
        meta.get("label")
            .or_else(|| meta.get("provider"))
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .unwrap_or(&self.key)
            .to_owned()
    }
    pub fn username(&self) -> String {
        self.metadata()
            .get("username")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if self.key.contains('@') && !self.key.contains(':') {
                    self.key.clone()
                } else {
                    self.value.clone()
                }
            })
    }
    pub fn title(&self) -> String {
        self.link
            .clone()
            .or_else(|| {
                self.metadata()
                    .get("password_title")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| self.key.clone())
    }
    pub fn remark(&self) -> String {
        let v = self.metadata();
        if v.is_object() {
            v.get(if v["type"] == "mail-account" {
                "purpose"
            } else {
                "text"
            })
            .and_then(Value::as_str)
            .unwrap_or("")
            .into()
        } else {
            self.note.clone()
        }
    }
}

impl Store {
    pub fn open(project: &Project) -> Result<Self> {
        let path = project.root.join("data/id.db");
        project.contained(&path)?;
        // SQLite 与 Web 共用，保留既有目录及文件权限。
        fs::create_dir_all(path.parent().unwrap())?;
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA foreign_keys=ON;")?;
        db.execute_batch(include_str!("../template/data/schema.sql"))?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS password_links(item_id INTEGER PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,entry_path TEXT NOT NULL);")?;
        Ok(Self { db })
    }
    pub fn items(&self, identity: &str) -> Result<Vec<Item>> {
        let mut statement=self.db.prepare("SELECT i.id,i.category,i.key,i.value,i.note,l.entry_path FROM items i LEFT JOIN password_links l ON l.item_id=i.id WHERE i.identity=? ORDER BY i.category,i.key")?;
        Ok(statement
            .query_map([identity], |r| {
                Ok(Item {
                    id: r.get(0)?,
                    category: r.get(1)?,
                    key: r.get(2)?,
                    value: r.get(3)?,
                    note: r.get(4)?,
                    link: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn put(
        &self,
        identity: &str,
        category: &str,
        key: &str,
        value: &str,
        note: &str,
    ) -> Result<()> {
        ensure!(
            [identity, category, key, value]
                .iter()
                .all(|v| !v.trim().is_empty()),
            "身份、分类、名称和内容不能为空"
        );
        self.db.execute("INSERT INTO items(identity,category,key,value,note) VALUES(?,?,?,?,?) ON CONFLICT(identity,category,key) DO UPDATE SET value=excluded.value,note=excluded.note,updated_at=datetime('now')",params![identity,item_category(category, key),key,value,note])?;
        Ok(())
    }
    pub fn event(&self, identity: &str, title: &str, detail: &str) -> Result<()> {
        ensure!(
            !identity.is_empty() && !title.trim().is_empty(),
            "身份和事件标题不能为空"
        );
        self.db.execute(
            "INSERT INTO events(identity,title,detail) VALUES(?,?,?)",
            params![identity, title, detail],
        )?;
        Ok(())
    }
    fn owned(&self, identity: &str, id: i64) -> Result<Item> {
        self.items(identity)?
            .into_iter()
            .find(|item| item.id == id)
            .context("该记录不属于当前身份")
    }
    pub fn link(&self, identity: &str, id: i64, path: Option<&str>) -> Result<()> {
        self.owned(identity, id)?;
        if let Some(path) = path {
            ensure!(
                !path.trim_matches('/').is_empty() && !path.chars().any(char::is_control),
                "密码条目路径无效"
            );
            self.db.execute("INSERT INTO password_links VALUES(?,?) ON CONFLICT(item_id) DO UPDATE SET entry_path=excluded.entry_path",params![id,path.trim_start_matches('/')])?;
        } else {
            self.db
                .execute("DELETE FROM password_links WHERE item_id=?", [id])?;
        }
        Ok(())
    }
    pub fn note(&self, identity: &str, id: i64, text: &str) -> Result<()> {
        let item = self.owned(identity, id)?;
        let mut value = item.metadata();
        let note = if value.is_object() {
            let key = if value["type"] == "mail-account" {
                "purpose"
            } else {
                "text"
            };
            value[key] = json!(text);
            value.to_string()
        } else {
            text.into()
        };
        self.db.execute(
            "UPDATE items SET note=?,updated_at=datetime('now') WHERE id=? AND identity=?",
            params![note, id, identity],
        )?;
        Ok(())
    }
    pub fn delete_field(&self, identity: &str, id: i64, field: &str) -> Result<()> {
        if field == "link" {
            return self.link(identity, id, None);
        }
        let item = self.owned(identity, id)?;
        ensure!(
            fields(&item.note).iter().any(|(key, _, _)| key == field),
            "此字段不能删除"
        );
        let mut value = item.metadata();
        let note = if let Some(map) = value.as_object_mut() {
            map.remove(field.trim_start_matches("meta:"));
            value.to_string()
        } else {
            String::new()
        };
        self.db.execute(
            "UPDATE items SET note=?,updated_at=datetime('now') WHERE id=? AND identity=?",
            params![note, id, identity],
        )?;
        Ok(())
    }
    pub fn delete(&mut self, identity: &str, id: i64) -> Result<()> {
        self.owned(identity, id)?;
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM password_links WHERE item_id=?", [id])?;
        tx.execute(
            "DELETE FROM items WHERE id=? AND identity=?",
            params![id, identity],
        )?;
        tx.execute(
            "INSERT INTO events(identity,title,detail) VALUES(?,'删除资料记录',?)",
            params![identity, format!("资料 ID {id}")],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn unlink_password(&self, identity: &str, title: &str) -> Result<()> {
        self.db.execute("DELETE FROM password_links WHERE entry_path=? AND item_id IN(SELECT id FROM items WHERE identity=?)",params![title,identity])?;
        Ok(())
    }
}

pub fn password_states(
    project: &Project,
    identity: &str,
    session: &Session,
) -> Result<BTreeMap<String, String>> {
    let vault = identity::vault(project, identity)?;
    let query = |operation: &str, args: Vec<String>| -> Result<Vec<String>> {
        let output = session.run(&vault, operation, &args, &[])?;
        Ok(std::str::from_utf8(&output)?
            .lines()
            .filter(|s| !s.is_empty() && !s.ends_with('/'))
            .map(|s| s.trim_start_matches('/').into())
            .collect())
    };
    let paths = query("ls", vec!["-R".into(), "-f".into()])?;
    let query_search = |pattern: &str| -> Result<Vec<String>> {
        let output = session.run(&vault, "search", &[pattern.into()], &[])?;
        Ok(std::str::from_utf8(&output)?
            .lines()
            .map(|s| s.trim_start_matches('/').into())
            .collect())
    };
    let empty = query_search("*pw:\\A\\z")?;
    let filled = query_search("*pw:[\\s\\S]")?;
    Ok(paths
        .into_iter()
        .filter(|s| {
            !(s.ends_with("/[empty]") || s == "[empty]") || empty.contains(s) || filled.contains(s)
        })
        .map(|p| {
            let status = if empty.contains(&p) && !filled.contains(&p) {
                "未填密码"
            } else if filled.contains(&p) && !empty.contains(&p) {
                "已设置"
            } else {
                "状态未知"
            };
            (p, status.into())
        })
        .collect())
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
    let title = if let Some(t) = title.filter(|v| !v.is_empty()) {
        t.to_owned()
    } else if edit {
        select_password(project, identity, "", session)?.context("已取消")?
    } else {
        ui::input("条目标题: ")?
    };
    ensure!(
        !title.trim().is_empty() && !title.chars().any(char::is_control),
        "条目标题无效"
    );
    let user = if edit {
        username.map(str::to_owned)
    } else {
        Some(match username {
            Some(user) => user.to_owned(),
            None => ui::input("用户名（可空）: ")?,
        })
    };
    session.unlock(&vault, edit)?;
    let mut args = vec![title.clone()];
    if let Some(user) = user {
        args.extend(["-u".into(), user]);
    }
    if generate {
        args.extend([
            "-g".into(),
            "-L".into(),
            "20".into(),
            "-l".into(),
            "-U".into(),
            "-n".into(),
            "-s".into(),
            "--every-group".into(),
        ]);
        session.run(&vault, if edit { "edit" } else { "add" }, &args, &[])?;
    } else {
        let password = ui::secret("新条目密码: ")?;
        args.push("-p".into());
        session.run(
            &vault,
            if edit { "edit" } else { "add" },
            &args,
            &[&password],
        )?;
    }
    println!("密码已保存。");
    Ok(title)
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

pub fn delete_password(
    project: &Project,
    identity: &str,
    title: &str,
    session: &Session,
    store: &Store,
) -> Result<()> {
    let vault = identity::vault(project, identity)?;
    session.unlock(&vault, true)?;
    let paths = || -> Result<Vec<String>> {
        let output = session.run(&vault, "ls", &["-R".into(), "-f".into()], &[])?;
        let mut paths = std::str::from_utf8(&output)?
            .lines()
            .filter(|s| !s.is_empty() && !s.ends_with('/'))
            .map(|s| s.trim_start_matches('/').to_owned())
            .collect::<Vec<_>>();
        if paths
            .iter()
            .any(|p| p.rsplit('/').next() == Some("[empty]"))
        {
            let states = password_states(project, identity, session)?;
            paths.retain(|p| p.rsplit('/').next() != Some("[empty]") || states.contains_key(p));
        }
        Ok(paths)
    };
    let before = paths()?;
    ensure!(
        before.iter().filter(|p| p.as_str() == title).count() == 1,
        "未找到唯一的条目路径，请在 KeePassXC 中检查重名条目"
    );
    let folder = project
        .root
        .join("backups/password-deletions")
        .join(identity);
    project.contained(&folder)?;
    private_dir(&folder)?;
    let path = folder.join(format!(
        "{}.kdbx",
        chrono::Local::now().format("%Y%m%d-%H%M%S-%f")
    ));
    let mut backup = private_file(&path)?;
    std::io::copy(&mut fs::File::open(&vault)?, &mut backup)?;
    backup.sync_all()?;
    session.run(&vault, "rm", &[title.into()], &[])?;
    store.unlink_password(identity, title)?;
    let after = paths()?;
    ensure!(
        !after.iter().any(|p| p == title),
        "删除未完成，加密备份已保留"
    );
    let mut counts = HashMap::new();
    for p in &before {
        *counts.entry(p).or_insert(0usize) += 1;
    }
    let mut added = Vec::new();
    for p in &after {
        let n = counts.entry(p).or_default();
        if *n > 0 {
            *n -= 1;
        } else {
            added.push(p);
        }
    }
    if !added.is_empty() {
        ensure!(
            added.len() == 1
                && !before.contains(added[0])
                && added[0].rsplit('/').next() == title.rsplit('/').next(),
            "条目已移入回收站，路径有歧义；请在 KeePassXC 中继续操作，加密备份已保留"
        );
        session.run(&vault, "rm", &[added[0].clone()], &[])?;
    }
    println!(
        "密码条目已删除；加密备份：{}",
        path.strip_prefix(&project.root)?.display()
    );
    Ok(())
}

#[derive(Clone)]
pub struct Record {
    pub item: Option<Item>,
    pub title: String,
    pub status: String,
    pub linked: usize,
}
impl Record {
    pub fn kind(&self) -> &str {
        match &self.item {
            Some(i) if i.category == "account" => "account",
            Some(_) => "info",
            None => "password",
        }
    }
    pub fn group(&self) -> String {
        match &self.item {
            Some(i) if i.category != "account" => {
                format!("个人信息 / {}", category_label(&i.category))
            }
            Some(_) => "账号".into(),
            None => "密码".into(),
        }
    }
    pub fn details(&self, items: &[Item]) -> Vec<(String, String)> {
        if let Some(i) = &self.item {
            let mut fields = if i.category == "account" {
                vec![("账号".into(), i.name()), ("登录名".into(), i.username())]
            } else {
                vec![
                    ("名称".into(), i.key.clone()),
                    ("内容".into(), i.value.clone()),
                ]
            };
            fields.extend(
                crate::store::fields(&i.note)
                    .into_iter()
                    .map(|(_, label, value)| (label, value)),
            );
            fields.push((
                "关联密码".into(),
                i.link.clone().unwrap_or_else(|| "—".into()),
            ));
            fields
        } else {
            vec![
                ("条目".into(), self.title.clone()),
                ("密码状态".into(), self.status.clone()),
                (
                    "关联资料".into(),
                    items
                        .iter()
                        .filter(|i| i.link.as_deref() == Some(&self.title))
                        .map(|i| format!("{}: {}", i.key, i.value))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            ]
        }
    }
}
pub fn records(
    items: &[Item],
    states: &BTreeMap<String, String>,
    view: &str,
    category: &str,
) -> Vec<Record> {
    let mut result = Vec::new();
    if view == "all" || view == "account" {
        for item in items.iter().filter(|i| i.category == "account") {
            let status = item
                .link
                .as_ref()
                .map(|p| states.get(p).map(String::as_str).unwrap_or("条目不存在"))
                .unwrap_or("未关联");
            result.push(Record {
                title: item.title(),
                item: Some(item.clone()),
                status: status.into(),
                linked: 0,
            });
        }
    }
    if view == "all" || view == "password" {
        for (title, status) in states {
            result.push(Record {
                item: None,
                title: title.clone(),
                status: status.clone(),
                linked: items
                    .iter()
                    .filter(|i| i.link.as_ref() == Some(title))
                    .count(),
            });
        }
    }
    if view == "all" || view == "info" {
        for item in items
            .iter()
            .filter(|i| i.category != "account" && (category.is_empty() || i.category == category))
        {
            result.push(Record {
                title: item.key.clone(),
                item: Some(item.clone()),
                status: String::new(),
                linked: 0,
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Project, Store) {
        let d = tempfile::tempdir().unwrap();
        let p = Project {
            root: d.path().canonicalize().unwrap(),
        };
        let s = Store::open(&p).unwrap();
        (d, p, s)
    }
    #[test]
    fn notes_links_and_deletes_are_scoped() {
        let (_d, _p, mut s) = fixture();
        s.put(
            "alice",
            "account",
            "GitHub",
            "alice",
            "{\"source\":\"local\",\"text\":\"备注\"}",
        )
        .unwrap();
        s.put("bob", "account", "GitHub", "bob", "").unwrap();
        let a = s.items("alice").unwrap()[0].id;
        let b = s.items("bob").unwrap()[0].id;
        assert!(s.delete("alice", b).is_err());
        assert!(s.link("alice", b, Some("GitHub")).is_err());
        s.link("alice", a, Some("GitHub")).unwrap();
        s.note("alice", a, "完整\n备注").unwrap();
        assert_eq!(s.items("alice").unwrap()[0].metadata()["source"], "local");
        assert!(s.delete_field("alice", a, "meta:source").is_err());
        s.delete_field("alice", a, "meta:text").unwrap();
        s.delete("alice", a).unwrap();
        assert!(s.items("alice").unwrap().is_empty());
        assert_eq!(s.items("bob").unwrap().len(), 1);
        assert_eq!(
            s.db.query_row("SELECT count(*) FROM password_links", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn inference_and_legacy_metadata_remain_compatible() {
        assert_eq!(infer_title("查一下我的github密码"), "github密码");
        let i = Item {
            id: 1,
            category: "account".into(),
            key: "a@example.com".into(),
            value: "proton 国外".into(),
            note: "{\"type\":\"mail-account\",\"provider\":\"Proton\",\"purpose\":\"完整备注\"}"
                .into(),
            link: None,
        };
        assert_eq!(i.username(), "a@example.com");
        assert_eq!(i.name(), "Proton");
        assert_eq!(i.remark(), "完整备注");
    }
    #[test]
    fn sql_values_are_parameters_and_invalid_rows_are_rejected() {
        let (_d, _p, s) = fixture();
        s.put("alice", "work", "O'Reilly", "$(literal)", "note")
            .unwrap();
        assert_eq!(s.items("alice").unwrap()[0].key, "O'Reilly");
        assert!(s.put("alice", "work", "x", "", "").is_err());
    }
}
