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
    Record(Record, char),
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
                    } else if row.kind() == "password" {
                        "  条目                              密码        关联资料"
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
                ui::clip(&row.title, width.saturating_sub(16)),
                ui::color(
                    &row.status,
                    if row.status == "已设置" {
                        "32"
                    } else {
                        "33"
                    }
                )
            )
        } else if let Some(item) = &row.item {
            if row.kind() == "account" {
                format!(
                    "{}  {}  {}  {}",
                    cell(&item.name(), 20),
                    cell(&item.username(), width.saturating_sub(58)),
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
                    cell(&item.key, 20),
                    cell(&item.value, width.saturating_sub(46)),
                    ui::clip(&item.remark(), 20)
                )
            }
        } else {
            format!(
                "{}  {}  {}",
                cell(&row.title, width.saturating_sub(28)),
                ui::color(
                    &cell(&row.status, 10),
                    if row.status == "已设置" {
                        "32"
                    } else {
                        "33"
                    }
                ),
                row.linked
            )
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
                    ui::color(&ui::clip(&format!("ID  /  {identity}"), width), "1;36"),
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
                        ui::color("e 修改  o 操作  f 删字段  ", "2"),
                        ui::color("x 删整条  q 退出", "31")
                    )
                } else {
                    ui::color(
                        &ui::clip(
                            "1 全部  2 账号  3 密码  4 信息  d 维度  r 刷新  q 退出",
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
                    KeyCode::Char('d') => return Ok(Action::Dimension),
                    KeyCode::Enter if !current.is_empty() => {
                        self.expanded = !self.expanded;
                        follow = true;
                    }
                    KeyCode::Char(c @ ('e' | 'o' | 'f' | 'x'))
                        if self.expanded && !current.is_empty() =>
                    {
                        return Ok(Action::Record(current[self.selected].clone(), c));
                    }
                    KeyCode::Delete if self.expanded && !current.is_empty() => {
                        return Ok(Action::Record(current[self.selected].clone(), 'x'));
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

pub fn list(
    project: &Project,
    identity: &str,
    mut view: String,
    mut category: String,
    session: &Session,
) -> Result<()> {
    let mut store = Store::open(project)?;
    let mut states = None;
    let mut browser = Browser::default();
    loop {
        let items = store.items(identity)?;
        if states.is_none()
            && (matches!(view.as_str(), "all" | "password")
                || (view == "account" && items.iter().any(|i| i.link.is_some())))
        {
            states = Some(store::password_states(project, identity, session)?);
        }
        let empty = BTreeMap::new();
        let records = store::records(&items, states.as_ref().unwrap_or(&empty), &view, &category);
        match browser.choose(identity, &view, &records, &items)? {
            Action::Exit => return Ok(()),
            Action::Reload => states = None,
            Action::Filter(next) => view = next,
            Action::Dimension => {
                let mut categories = vec![String::new()];
                for i in &items {
                    if i.category != "account" && !categories.contains(&i.category) {
                        categories.push(i.category.clone());
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
            Action::Record(row, mut action) => {
                if action == 'o' {
                    let options = if row.item.is_some() {
                        vec![
                            "编辑备注",
                            "关联密码",
                            "新增密码并关联",
                            "修改关联密码",
                            "删除单个字段",
                            "删除整条记录",
                        ]
                    } else {
                        vec!["修改密码", "删除整条密码"]
                    };
                    let options = options.iter().map(|s| s.to_string()).collect::<Vec<_>>();
                    let Some(i) = ui::choose(&row.title, &options)? else {
                        continue;
                    };
                    action = if row.item.is_some() {
                        ['e', 'l', 'a', 'p', 'f', 'x'][i]
                    } else {
                        ['e', 'x'][i]
                    };
                }
                let operation = (|| -> Result<()> {
                    if let Some(item) = &row.item {
                        match action {
                            'e' => {
                                let text = ui::input("新备注（Enter 保留，- 清空）: ")?;
                                if !text.is_empty() {
                                    store.note(
                                        identity,
                                        item.id,
                                        if text == "-" { "" } else { &text },
                                    )?;
                                }
                            }
                            'l' => {
                                if let Some(title) =
                                    store::select_password(project, identity, "", session)?
                                {
                                    store.link(identity, item.id, Some(&title))?;
                                }
                            }
                            'a' => {
                                let title = store::write_password(
                                    project,
                                    identity,
                                    Some(&item.title()),
                                    Some(&item.username()),
                                    false,
                                    false,
                                    session,
                                )?;
                                store.link(identity, item.id, Some(&title))?;
                                states = None;
                            }
                            'p' => {
                                if let Some(title) = &item.link {
                                    store::write_password(
                                        project,
                                        identity,
                                        Some(title),
                                        None,
                                        true,
                                        false,
                                        session,
                                    )?;
                                    states = None;
                                }
                            }
                            'f' => {
                                let mut fields = store::fields(&item.note);
                                if item.link.is_some() {
                                    fields.push(("link".into(), "密码关联".into(), String::new()));
                                }
                                let mut labels = vec!["取消".into()];
                                labels.extend(fields.iter().map(|(_, label, _)| label.clone()));
                                if let Some(index) = ui::choose("选择要删除的字段", &labels)?
                                {
                                    if index > 0
                                        && ui::confirm(&format!(
                                            "删除 [{}] {} 的{}？",
                                            identity,
                                            item.name(),
                                            fields[index - 1].1
                                        ))?
                                    {
                                        store.delete_field(
                                            identity,
                                            item.id,
                                            &fields[index - 1].0,
                                        )?;
                                    }
                                }
                            }
                            'x' if ui::confirm(&format!(
                                "删除 [{}] {}？关联的密码条目保留。",
                                identity,
                                item.name()
                            ))? =>
                            {
                                store.delete(identity, item.id)?;
                                browser.expanded = false;
                            }
                            _ => {}
                        }
                    } else {
                        match action {
                            'e' => {
                                store::write_password(
                                    project,
                                    identity,
                                    Some(&row.title),
                                    None,
                                    true,
                                    false,
                                    session,
                                )?;
                                states = None;
                            }
                            'x' if ui::confirm(&format!(
                                "删除 [{}] {}？先备份密码库，并再次验证主密码。",
                                identity, row.title
                            ))? =>
                            {
                                store::delete_password(
                                    project, identity, &row.title, session, &store,
                                )?;
                                states = None;
                                browser.expanded = false;
                            }
                            _ => {}
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pagination_keeps_all_records_and_expansion_keeps_full_notes() {
        let item = Item {
            id: 1,
            category: "work".into(),
            key: "职位".into(),
            value: "工程师".into(),
            note: "长备注\n".repeat(40),
            link: None,
        };
        let records = vec![
            Record {
                item: Some(item.clone()),
                title: "职位".into(),
                status: String::new(),
                linked: 0
            };
            40
        ];
        let pages = batches(&records, 24);
        assert_eq!(pages.iter().map(|p| p.len()).sum::<usize>(), 40);
        let (body, _) = render(&records[..1], &[item], 0, true, 40);
        assert_eq!(body.iter().filter(|l| l.contains("长备注")).count(), 40);
        let details = body.iter().position(|l| l.contains("┌ 详情")).unwrap();
        assert!(!body[details..].iter().any(|l| l.contains("…")));
    }
}
