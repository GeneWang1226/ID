use crate::{
    Project,
    store::{self, Item, Record, Store},
    ui,
    vault::Session,
};
use anyhow::Result;
use crossterm::{event::KeyCode, terminal};
use std::{collections::BTreeMap, time::Duration};
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
struct Browser {
    page: usize,
    selected: usize,
    expanded: bool,
    offset: usize,
}
enum Action {
    Exit,
    Reload,
    Filter(String),
    Dimension,
    New,
    Lock,
    Record(usize, char),
}
fn batches(records: &[Record], height: usize) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut cost = 0;
    let mut group = String::new();
    for (i, row) in records.iter().enumerate() {
        let label = row.group();
        let needed = if label == group { 1 } else { 4 };
        if i > start && cost + needed > height.saturating_sub(8) {
            ranges.push(start..i);
            start = i;
            cost = 0;
            group.clear();
        }
        cost += if label == group { 1 } else { 4 };
        group = label;
    }
    ranges.push(start..records.len());
    ranges
}
fn cell(value: &str, size: usize) -> String {
    let value = ui::clip(value, size);
    let pad = size.saturating_sub(value.width());
    format!("{value}{}", " ".repeat(pad))
}
fn render(
    records: &[Record],
    items: &[Item],
    selected: usize,
    expanded: bool,
    width: usize,
) -> (Vec<String>, usize) {
    let mut body = Vec::new();
    let mut group = String::new();
    let mut position = 0;
    for (i, row) in records.iter().enumerate() {
        let label = row.group();
        if label != group {
            if !body.is_empty() {
                body.push(String::new());
            }
            body.push(ui::color(&ui::clip(&label, width), "1"));
            body.push(ui::color(
                &ui::clip(
                    if row.kind() == "account" {
                        "  账号                  登录名 / 邮箱              密码        备注"
                    } else {
                        "  名称                  内容                      备注"
                    },
                    width,
                ),
                "2",
            ));
            group = label;
        }
        let content = if width < 60 {
            format!(
                "{}  {}",
                ui::clip(
                    &format!("{} {}", row.item.glyph(), row.item.name()),
                    width.saturating_sub(16)
                ),
                ui::color(
                    &row.status,
                    if row.status == "已设置" {
                        "32"
                    } else {
                        "33"
                    }
                )
            )
        } else {
            let item = &row.item;
            if row.kind() == "account" {
                format!(
                    "{}  {}  {}  {}",
                    cell(&format!("{} {}", item.glyph(), item.name()), 20),
                    cell(&item.username(), width.saturating_sub(58).min(48)),
                    ui::color(
                        &cell(&row.status, 10),
                        if row.status == "已设置" {
                            "32"
                        } else {
                            "33"
                        }
                    ),
                    ui::clip(&item.remark(), 20)
                )
            } else {
                format!(
                    "{}  {}  {}",
                    cell(&format!("{} {}", item.glyph(), item.key), 20),
                    cell(
                        &if item.category == "info" {
                            format!("{} 个字段", item.fields.len())
                        } else {
                            item.value.clone()
                        },
                        width.saturating_sub(46).min(60)
                    ),
                    ui::clip(&item.remark(), 20)
                )
            }
        };
        if i == selected {
            position = body.len();
            body.push(format!("{} {content}", ui::color("›", "1;36")));
        } else {
            body.push(format!("  {content}"));
        }
        if i == selected && expanded {
            body.push(ui::color("    ┌ 详情", "2"));
            for (label, value) in row.details(items) {
                body.push(ui::color(
                    &format!("    {}", ui::clip(&label, width.saturating_sub(4))),
                    "36",
                ));
                for line in ui::wrap(
                    if value.is_empty() { "—" } else { &value },
                    width.saturating_sub(8),
                ) {
                    body.push(format!("      {line}"));
                }
            }
            body.push(ui::color("    └", "2"));
        }
    }
    if records.is_empty() {
        body.push(ui::color("  暂无记录", "2"));
    }
    (body, position)
}
impl Browser {
    fn choose(
        &mut self,
        identity: &str,
        view: &str,
        records: &[Record],
        items: &[Item],
    ) -> Result<Action> {
        let _terminal = ui::Terminal::new(true)?;
        let mut follow = true;
        loop {
            let (w, h) = terminal::size()?;
            let (width, height) = (w as usize, h as usize);
            if width < 30 || height < 12 {
                ui::draw(&[ui::clip("请扩大终端，q 退出", width)])?;
            } else {
                let pages = batches(records, height);
                self.page = self.page.min(pages.len() - 1);
                let current = &records[pages[self.page].clone()];
                self.selected = self.selected.min(current.len().saturating_sub(1));
                let mut lines = vec![
                    ui::color(
                        &ui::clip(&format!("ID  /  身份管理  /  {identity}"), width),
                        "1;36",
                    ),
                    String::new(),
                    [
                        ("all", "全部"),
                        ("account", "账号"),
                        ("password", "密码"),
                        ("info", "个人信息"),
                    ]
                    .iter()
                    .map(|(key, label)| ui::color(label, if *key == view { "1;36" } else { "2" }))
                    .collect::<Vec<_>>()
                    .join("   "),
                    String::new(),
                ];
                let (body, position) = render(current, items, self.selected, self.expanded, width);
                let available = height - 8;
                if follow {
                    self.offset = if self.expanded {
                        position.saturating_sub(2)
                    } else {
                        0
                    };
                    follow = false;
                }
                self.offset = self.offset.min(body.len().saturating_sub(available));
                lines.extend(body.into_iter().skip(self.offset).take(available));
                lines.push(ui::color(
                    &format!(
                        "第 {}/{} 页 · {} 条",
                        self.page + 1,
                        pages.len(),
                        records.len()
                    ),
                    "2",
                ));
                lines.push(ui::color(
                    &ui::clip(
                        if self.expanded {
                            "↑↓ 选择  ←→ 翻页  PgUp/PgDn 滚动  Enter 收起"
                        } else {
                            "↑↓ 选择  ←→/PgUp/PgDn 翻页  Enter 展开"
                        },
                        width,
                    ),
                    "2",
                ));
                lines.push(if self.expanded {
                    format!(
                        "{}{}",
                        ui::color("c 字段  e 修改  o 操作  f 删字段  ", "2"),
                        ui::color("x 删整条  q 退出", "31")
                    )
                } else {
                    ui::color(
                        &ui::clip(
                            "1 全部  2 账号  3 密码  4 信息  n 新增  d 维度  r 刷新  l 锁定  q 退出",
                            width,
                        ),
                        "2",
                    )
                });
                ui::draw(&lines)?;
            }
            if let Some(k) = ui::key(Duration::from_millis(200))? {
                if ui::cancelled(k) || k.code == KeyCode::Char('q') {
                    return Ok(Action::Exit);
                }
                let pages = batches(records, height);
                self.page = self.page.min(pages.len() - 1);
                let current = &records[pages[self.page].clone()];
                match k.code {
                    KeyCode::Char(c @ '1'..='4') => {
                        self.page = 0;
                        self.selected = 0;
                        self.expanded = false;
                        return Ok(Action::Filter(
                            ["all", "account", "password", "info"][(c as u8 - b'1') as usize]
                                .into(),
                        ));
                    }
                    KeyCode::Char('r') => return Ok(Action::Reload),
                    KeyCode::Char('n') => return Ok(Action::New),
                    KeyCode::Char('l') => return Ok(Action::Lock),
                    KeyCode::Char('d') => return Ok(Action::Dimension),
                    KeyCode::Enter if !current.is_empty() => {
                        self.expanded = !self.expanded;
                        follow = true;
                    }
                    KeyCode::Char(c @ ('e' | 'o' | 'f' | 'x' | 'c'))
                        if self.expanded && !current.is_empty() =>
                    {
                        return Ok(Action::Record(pages[self.page].start + self.selected, c));
                    }
                    KeyCode::Delete if self.expanded && !current.is_empty() => {
                        return Ok(Action::Record(pages[self.page].start + self.selected, 'x'));
                    }
                    KeyCode::PageDown if self.expanded => {
                        self.offset += height.saturating_sub(10).max(1)
                    }
                    KeyCode::PageUp if self.expanded => {
                        self.offset = self.offset.saturating_sub(height.saturating_sub(10).max(1))
                    }
                    KeyCode::Right | KeyCode::PageDown => {
                        self.page = (self.page + 1).min(pages.len() - 1);
                        self.selected = 0;
                        self.expanded = false;
                        follow = true;
                    }
                    KeyCode::Left | KeyCode::PageUp => {
                        self.page = self.page.saturating_sub(1);
                        self.selected = 0;
                        self.expanded = false;
                        follow = true;
                    }
                    KeyCode::Down => {
                        if self.selected + 1 < current.len() {
                            self.selected += 1;
                        } else if self.page + 1 < pages.len() {
                            self.page += 1;
                            self.selected = 0;
                        }
                        self.expanded = false;
                        follow = true;
                    }
                    KeyCode::Up => {
                        if self.selected > 0 {
                            self.selected -= 1;
                        } else if self.page > 0 {
                            self.page -= 1;
                            self.selected = usize::MAX;
                        }
                        self.expanded = false;
                        follow = true;
                    }
                    _ => {}
                }
            }
        }
    }
}

pub fn manage(
    project: &Project,
    identity: &str,
    mut view: String,
    mut category: String,
    session: &Session,
) -> Result<()> {
    let _terminal = ui::Terminal::new(true)?;
    let store = Store::open(project, identity, session)?;
    let mut browser = Browser::default();
    loop {
        let items = store.items(identity)?;
        let records = store::records(&items, &BTreeMap::new(), &view, &category);
        let action = browser.choose(identity, &view, &records, &items)?;
        if matches!(action, Action::Exit) {
            return Ok(());
        }
        let operation = (|| -> Result<()> {
            match action {
                Action::Exit => return Ok(()),
                Action::Reload => {}
                Action::Lock => {
                    session.lock();
                    ui::input("已锁定，回车重新解锁: ")?;
                }
                Action::Filter(next) => view = next,
                Action::New => new_record(&store)?,
                Action::Dimension => {
                    let mut categories = vec![String::new()];
                    for f in items.iter().flat_map(|i| &i.fields) {
                        if !f.category.is_empty() && !categories.contains(&f.category) {
                            categories.push(f.category.clone());
                        }
                    }
                    let labels = categories
                        .iter()
                        .map(|c| {
                            if c.is_empty() {
                                "全部维度".into()
                            } else {
                                store::category_label(c).into()
                            }
                        })
                        .collect::<Vec<_>>();
                    if let Some(i) = ui::choose("选择维度", &labels)? {
                        category = categories[i].clone();
                        view = "info".into();
                        browser = Browser::default();
                    }
                }
                Action::Record(index, mut key) => {
                    let item = &records[index].item;
                    if key == 'o' {
                        let mut choices = vec![
                            "编辑备注",
                            "编辑字段",
                            "添加字段",
                            "设置密码",
                            "复制密码",
                            "修改图标",
                            "导入附件",
                            "导出附件",
                            "删除字段",
                            "删除整条",
                            "取用字段",
                        ];
                        if item.category == "account" {
                            choices.push(if item.email {
                                "邮箱常用属性"
                            } else {
                                "归为邮箱"
                            });
                        }
                        let Some(i) = ui::choose(
                            &item.key,
                            &choices.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                        )?
                        else {
                            return Ok(());
                        };
                        key = ['e', 'v', 'a', 'p', 'P', 'i', 't', 's', 'f', 'x', 'c', 'm'][i];
                    }
                    match key {
                        'e' => {
                            let text = ui::input("新备注（Enter 保留，- 清空）: ")?;
                            if !text.is_empty() {
                                store.note(
                                    identity,
                                    &item.id,
                                    if text == "-" { "" } else { &text },
                                )?;
                            }
                        }
                        'a' => edit_field(&store, item, None)?,
                        'v' => {
                            let mut keys = if item.category == "account" {
                                vec!["UserName".to_string(), "URL".into()]
                            } else {
                                Vec::new()
                            };
                            keys.extend(item.fields.iter().map(|f| f.name.clone()));
                            let labels = keys
                                .iter()
                                .map(|s| match s.as_str() {
                                    "UserName" => "登录名".into(),
                                    "URL" => "网址".into(),
                                    _ => s.clone(),
                                })
                                .collect::<Vec<_>>();
                            if let Some(i) = ui::choose("编辑字段", &labels)? {
                                edit_field(&store, item, Some(&keys[i]))?;
                            }
                        }
                        'p' if item.category == "account" => {
                            store::write_password(
                                project,
                                identity,
                                Some(&item.title()),
                                None,
                                item.password_set,
                                false,
                                session,
                            )?;
                        }
                        'P' if item.category == "account" => crate::clipboard::pass(
                            project,
                            identity,
                            Some(&item.title()),
                            "Password",
                            false,
                            session,
                        )?,
                        'c' => use_field(&store, item)?,
                        'm' if item.email => {
                            let choices = store::EMAIL_FIELDS
                                .iter()
                                .map(|(name, secret)| {
                                    format!("{name}{}", if *secret { " · 受保护" } else { "" })
                                })
                                .collect::<Vec<_>>();
                            if let Some(i) = ui::choose("邮箱常用属性", &choices)? {
                                edit_field(&store, item, Some(store::EMAIL_FIELDS[i].0))?;
                            }
                        }
                        'm' if item.category == "account"
                            && ui::confirm(
                                "归为邮箱并移入 ID_emails？保留已有密码、字段与附件。",
                            )? =>
                        {
                            store.email(&item.id)?
                        }
                        'i' => {
                            let labels = store::ICONS
                                .iter()
                                .map(|(i, s)| format!("{} {s}", store::icon(*i)))
                                .collect::<Vec<_>>();
                            if let Some(i) =
                                ui::choose("条目图标（同步 KeePass 标准图标）", &labels)?
                            {
                                store.icon(&item.id, store::ICONS[i].0)?;
                            }
                        }
                        't' | 's' => attachment(&store, item, key == 't')?,
                        'f' => {
                            let mut keys = item
                                .fields
                                .iter()
                                .map(|f| f.name.clone())
                                .collect::<Vec<_>>();
                            if !item.note.is_empty() {
                                keys.push("Notes".into());
                            }
                            if let Some(i) = ui::choose("删除字段", &keys)?
                                && ui::confirm(&format!("删除 {} 的 {}？", item.key, keys[i]))?
                            {
                                store.delete_field(identity, &item.id, &keys[i])?;
                            }
                        }
                        'x' if ui::confirm(&format!(
                            "删除 {} 及其密码、字段与附件？将保留加密备份。",
                            item.key
                        ))? =>
                        {
                            store.delete(identity, &item.id)?;
                            browser.expanded = false;
                        }
                        _ => {}
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = operation {
            eprintln!("{}", ui::color(&ui::clean(&error.to_string()), "31"));
            let _ = ui::input("回车返回 ");
        }
    }
}
fn use_field(store: &Store, item: &Item) -> Result<()> {
    anyhow::ensure!(
        !item.fields.is_empty(),
        "此条目没有自定义字段，可在操作菜单添加"
    );
    let choices = item
        .fields
        .iter()
        .map(|f| {
            format!(
                "{}{}",
                f.name,
                if f.value.is_none() {
                    " · 受保护"
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>();
    let Some(index) = ui::choose("选择字段", &choices)? else {
        return Ok(());
    };
    let field = &item.fields[index].name;
    let Some(action) = ui::choose(field, &["复制字段".into(), "临时查看".into()])? else {
        return Ok(());
    };
    crate::clipboard::pass(
        store.project,
        store.identity,
        Some(&item.title()),
        field,
        action == 1,
        store.session,
    )
}
fn new_record(store: &Store) -> Result<()> {
    let Some(kind) = ui::choose(
        "新增",
        &[
            "账号".into(),
            "基础信息字段".into(),
            "事件".into(),
            "邮箱".into(),
        ],
    )?
    else {
        return Ok(());
    };
    let name = ui::input(if kind == 1 {
        "字段名称: "
    } else {
        "名称: "
    })?;
    if name.is_empty() {
        return Ok(());
    }
    if kind == 1 && store::sensitive(&name) {
        anyhow::bail!("机密请在条目内选择添加字段，使用受保护输入");
    }
    let value = ui::input(if kind == 3 {
        "邮箱地址: "
    } else if kind == 0 {
        "登录名 / 邮箱 / 手机号: "
    } else {
        "内容: "
    })?;
    let note = ui::input("备注: ")?;
    match kind {
        0 => store.create_account(&name, &value, &note)?,
        3 => store.create_email(&name, &value, &note)?,
        1 => {
            let cat = ui::input("维度（例如 id-info / contact / work）: ")?;
            store.put(
                store.identity,
                if cat.is_empty() { "id-info" } else { &cat },
                &name,
                &value,
                &note,
            )?;
        }
        _ => store.event(store.identity, &name, &format!("{value}\n{note}"))?,
    }
    Ok(())
}
fn edit_field(store: &Store, item: &Item, key: Option<&str>) -> Result<()> {
    let key = match key {
        Some(k) => k.to_string(),
        None => ui::input("字段名称: ")?,
    };
    if key.is_empty() {
        return Ok(());
    }
    anyhow::ensure!(
        item.category != "info"
            || !matches!(
                key.as_str(),
                "UserName" | "URL" | "Title" | "Password" | "Notes"
            ),
        "基础信息请使用自定义字段名"
    );
    let existing = item.fields.iter().find(|f| f.name == key);
    let protected = store::sensitive(&key) || existing.is_some_and(|f| f.value.is_none());
    let secret = if protected {
        true
    } else {
        let Some(kind) = ui::choose("字段类型", &["普通文字".into(), "受保护内容".into()])?
        else {
            return Ok(());
        };
        kind == 1
    };
    let password = if secret {
        store.session.unlock(
            &crate::identity::vault(store.project, store.identity)?,
            true,
        )?;
        Some(ui::secret("字段内容: ")?)
    } else {
        None
    };
    let plain = if secret {
        String::new()
    } else {
        ui::input("字段内容: ")?
    };
    let value = match &password {
        Some(p) => std::str::from_utf8(p.bytes())?,
        None => &plain,
    };
    let category = if item.category == "info" {
        ui::input(if existing.is_some() {
            "维度（Enter 保留，- 清空）: "
        } else {
            "维度: "
        })?
    } else {
        String::new()
    };
    let note = ui::input(if existing.is_some() {
        "字段备注（Enter 保留，- 清空）: "
    } else {
        "字段备注: "
    })?;
    let keep = |input: String, old: &str| {
        if input == "-" {
            String::new()
        } else if input.is_empty() {
            old.into()
        } else {
            input
        }
    };
    let category = keep(
        category,
        existing.map(|f| f.category.as_str()).unwrap_or(""),
    );
    let note = keep(note, existing.map(|f| f.note.as_str()).unwrap_or(""));
    store.field(&item.id, &key, value, secret, &category, &note)
}
fn attachment(store: &Store, item: &Item, import: bool) -> Result<()> {
    use crate::kdbx;
    let source = if import {
        let Some(path) = crate::file_picker::choose(&store.project.root)? else {
            return Ok(());
        };
        Some(path)
    } else {
        None
    };
    let vault = crate::identity::vault(store.project, store.identity)?;
    store.session.unlock(&vault, true)?;
    let _lock = kdbx::WriteLock::acquire(store.project, store.identity)?;
    let (operation, args) = if let Some(source) = source {
        anyhow::ensure!(source.is_file(), "文件不存在");
        let name = source
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("文件名无效"))?;
        if item.attachments.iter().any(|n| n == name)
            && !ui::confirm("覆盖同名附件？将保留加密备份。")?
        {
            return Ok(());
        }
        kdbx::backup(store.project, store.identity)?;
        (
            "attachment-import",
            vec![
                item.title(),
                name.into(),
                source.to_string_lossy().into_owned(),
            ],
        )
    } else {
        let Some(i) = ui::choose("导出附件", &item.attachments)? else {
            return Ok(());
        };
        if !ui::confirm("附件将解密写入项目 exports 目录，继续？")? {
            return Ok(());
        }
        let name = &item.attachments[i];
        anyhow::ensure!(
            std::path::Path::new(name)
                .file_name()
                .is_some_and(|n| n == std::ffi::OsStr::new(name)),
            "附件名称包含路径，请用 KeePassXC 导出"
        );
        let folder = store.project.root.join("exports").join(store.identity);
        store.project.contained(&folder)?;
        crate::private_dir(&folder)?;
        let target = folder.join(name);
        store.project.contained(&target)?;
        anyhow::ensure!(!target.exists(), "目标文件已存在，未覆盖");
        (
            "attachment-export",
            vec![
                item.title(),
                name.into(),
                target.to_string_lossy().into_owned(),
            ],
        )
    };
    store.session.run(&vault, operation, &args, &[])?;
    #[cfg(unix)]
    if !import {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&args[2], std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pagination_and_expansion_keep_complete_native_notes() {
        let xml = format!(
            "<KeePassFile><Root><Group><Entry><UUID>test</UUID><IconID>9</IconID><String><Key>Title</Key><Value>基础信息</Value></String><String><Key>Notes</Key><Value>{}</Value></String></Entry></Group></Root></KeePassFile>",
            "长备注\n".repeat(40)
        );
        let document = crate::kdbx::Document {
            root: crate::kdbx::parse(xml.as_bytes()).unwrap(),
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        let items = store::snapshot(&document);
        let one = store::records(&items, &BTreeMap::new(), "all", "")[0].clone();
        let records = vec![one; 40];
        let pages = batches(&records, 24);
        assert_eq!(pages.iter().map(|r| r.len()).sum::<usize>(), 40);
        let (body, _) = render(&records[..1], &items, 0, true, 40);
        assert_eq!(body.iter().filter(|l| l.contains("长备注")).count(), 40);
        let start = body.iter().position(|l| l.contains("┌ 详情")).unwrap();
        assert!(!body[start..].iter().any(|l| l.contains('…')));
    }
}
