use crate::{
    Project, identity,
    store::{self, Store},
    ui,
    vault::Session,
};
use anyhow::{Context, Result, ensure};
use quick_xml::{Reader, events::Event};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{Cursor, Read},
    path::Path,
};
use zeroize::Zeroizing;

const HEADERS: [&str; 7] = [
    "分类",
    "服务商",
    "使用者",
    "账号",
    "备注",
    "密码备注",
    "详细数据",
];
struct Row {
    number: usize,
    public: [String; 5],
    secret: Vec<Zeroizing<String>>,
}
fn part(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Zeroizing<Vec<u8>>> {
    let file = archive.by_name(name)?;
    ensure!(file.size() <= 16 * 1024 * 1024, "表格过大，请先拆分");
    let mut out = Zeroizing::new(Vec::with_capacity(file.size() as usize + 1));
    file.take(16 * 1024 * 1024 + 1).read_to_end(&mut out)?;
    ensure!(out.len() <= 16 * 1024 * 1024, "表格过大，请先拆分");
    Ok(out)
}
fn read_sheet(bytes: &[u8], secret_row: Option<usize>) -> Result<Vec<Row>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut shared = Vec::<Zeroizing<String>>::new();
    if zip.by_name("xl/sharedStrings.xml").is_ok() {
        let xml = part(&mut zip, "xl/sharedStrings.xml")?;
        let mut reader = Reader::from_reader(&xml[..]);
        let mut current = Zeroizing::new(String::new());
        let mut inside = false;
        loop {
            let event = reader.read_event()?;
            match &event {
                Event::Start(e) if e.name().as_ref() == b"si" => {
                    current = Zeroizing::new(String::new());
                    inside = true;
                }
                Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) if inside => {
                    current.push_str(&crate::xml_text(&event)?.unwrap())
                }
                Event::End(e) if e.name().as_ref() == b"si" => {
                    shared.push(current);
                    current = Zeroizing::new(String::new());
                    inside = false;
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    let xml = part(&mut zip, "xl/worksheets/sheet1.xml")?;
    let mut reader = Reader::from_reader(&xml[..]);
    let mut cells: [Zeroizing<String>; 7] = std::array::from_fn(|_| Zeroizing::new(String::new()));
    let (mut number, mut column, mut shared_cell, mut text_value, mut first) =
        (0usize, 7usize, false, false, true);
    let mut rows = Vec::new();
    let email = regex::Regex::new(r"^[^@\s/\\]+@[^@\s/\\]+\.[^@\s/\\]+$")?;
    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(e) if e.name().as_ref() == b"row" => {
                number += 1;
                for a in e.attributes() {
                    let a = a?;
                    if a.key.as_ref() == b"r" {
                        number = std::str::from_utf8(&a.value)?.parse()?;
                    }
                }
                cells = std::array::from_fn(|_| Zeroizing::new(String::new()));
            }
            Event::Start(e) if e.name().as_ref() == b"c" => {
                column = 7;
                shared_cell = false;
                for a in e.attributes() {
                    let a = a?;
                    match a.key.as_ref() {
                        b"r" => {
                            let reference = std::str::from_utf8(&a.value)?;
                            let letters: String = reference
                                .chars()
                                .take_while(|c| c.is_ascii_alphabetic())
                                .collect();
                            if letters.len() == 1 {
                                column = (letters.as_bytes()[0] - b'A') as usize;
                            }
                        }
                        b"t" => shared_cell = a.value.as_ref() == b"s",
                        _ => {}
                    }
                }
            }
            Event::Start(e) if e.name().as_ref() == b"f" && column < 5 => {
                anyhow::bail!("不导入公式，请先转为值")
            }
            Event::Start(e) if matches!(e.name().as_ref(), b"t" | b"v") => text_value = true,
            Event::End(e) if matches!(e.name().as_ref(), b"t" | b"v") => text_value = false,
            Event::Text(_) | Event::CData(_) | Event::GeneralRef(_)
                if text_value
                    && column < 7
                    && (column < 5
                        || first
                        || (secret_row == Some(number) || secret_row == Some(0))) =>
            {
                cells[column].push_str(&crate::xml_text(&event)?.unwrap())
            }
            Event::End(e) if e.name().as_ref() == b"c" => {
                if column < 7 && shared_cell && !cells[column].is_empty() {
                    let i = cells[column].parse::<usize>()?;
                    let value = shared.get(i).context("共享字符串索引无效")?;
                    cells[column] = Zeroizing::new((**value).clone());
                }
                column = 7;
            }
            Event::End(e) if e.name().as_ref() == b"row" => {
                if first {
                    ensure!(
                        cells
                            .iter()
                            .zip(HEADERS)
                            .all(|(cell, label)| cell.trim() == label),
                        "表头须为：分类、服务商、使用者、账号、备注、密码备注、详细数据"
                    );
                    first = false;
                    continue;
                }
                let public: [String; 5] = std::array::from_fn(|i| cells[i].trim().to_owned());
                if public.iter().all(String::is_empty) {
                    continue;
                }
                ensure!(
                    public[..4].iter().all(|s| !s.chars().any(char::is_control)),
                    "资料包含终端控制字符"
                );
                ensure!(
                    public[3].len() <= 254 && email.is_match(&public[3]),
                    "邮箱地址无效"
                );
                let secret = if secret_row == Some(number) || secret_row == Some(0) {
                    vec![
                        std::mem::replace(&mut cells[5], Zeroizing::new(String::new())),
                        std::mem::replace(&mut cells[6], Zeroizing::new(String::new())),
                    ]
                } else {
                    Vec::new()
                };
                rows.push(Row {
                    number,
                    public,
                    secret,
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(!first, "表格为空");
    Ok(rows)
}

pub fn accounts(
    project: &Project,
    source: &Path,
    owners: &[String],
    session: &Session,
) -> Result<()> {
    let bytes = Zeroizing::new(fs::read(source)?);
    let rows = read_sheet(&bytes, Some(0))?;
    let source_hash = format!("{:x}", Sha256::digest(&*bytes));
    let mut aliases = identity::names(project)?
        .into_iter()
        .map(|n| (n.to_lowercase(), n))
        .collect::<HashMap<_, _>>();
    for pair in owners {
        let (alias, name) = pair
            .split_once('=')
            .context("归属映射格式：原使用者=身份")?;
        identity::select(project, Some(name))?;
        aliases.insert(alias.to_lowercase(), name.into());
    }
    let mut ready = BTreeMap::<String, Vec<Row>>::new();
    let mut pending = 0;
    let mut seen = HashSet::new();
    for row in rows {
        if let Some(owner) = aliases.get(&row.public[2].to_lowercase()) {
            ensure!(
                seen.insert((
                    owner.clone(),
                    row.public[1].to_lowercase(),
                    row.public[3].to_lowercase()
                )),
                "同一身份有重复账号，请先合并"
            );
            ready.entry(owner.clone()).or_default().push(row);
        } else {
            pending += 1;
        }
    }
    ensure!(!ready.is_empty(), "没有归属明确的账号，未写入");
    for (owner, rows) in ready {
        println!("导入身份 {owner}：{} 个账号", rows.len());
        let store = Store::open(project, &owner, session)?;
        store.update(|d| {
            for row in &rows {
                let [region, provider, _, account, purpose] = &row.public;
                let marker = format!("{source_hash}:{}", row.number);
                if d.entries()
                    .iter()
                    .any(|(_, e)| e.meta("ID.Import") == marker)
                {
                    continue;
                }
                let found = d
                    .entries()
                    .into_iter()
                    .filter(|(_, e)| {
                        let title = e.field_value("Title");
                        e.field_value("UserName").eq_ignore_ascii_case(account)
                            && ((!provider.is_empty()
                                && (title.eq_ignore_ascii_case(provider)
                                    || e.field_value("服务商").eq_ignore_ascii_case(provider)))
                                || title.eq_ignore_ascii_case(account)
                                || title.eq_ignore_ascii_case(&format!("邮箱 - {account}")))
                    })
                    .map(|(_, e)| e.value("UUID").to_owned())
                    .collect::<Vec<_>>();
                ensure!(found.len() <= 1, "存在重复服务账号，请先整理密码库");
                let id = if let Some(id) = found.first() {
                    id.clone()
                } else {
                    let base = if provider.is_empty() {
                        account.as_str()
                    } else {
                        provider.as_str()
                    };
                    let title = if d.find_entry(base)?.is_some() {
                        format!("{base} · {account}")
                    } else {
                        base.into()
                    };
                    ensure!(d.find_entry(&title)?.is_none(), "导入标题冲突，未覆盖");
                    let id = d.add(&title, "email", 19)?;
                    d.entry_mut(&id)?.set_field("UserName", account, false);
                    id
                };
                let e = d.entry_mut(&id)?;
                e.set_meta("ID.Import", &marker);
                // Imported comments may contain password hints; keep them protected and out of AI queries.
                e.set_field("原表备注", purpose, true);
                e.set_field("地区", region, false);
                e.set_field("服务商", provider, false);
                for (name, value) in ["密码备注", "详细数据"].into_iter().zip(&row.secret) {
                    if !value.is_empty() {
                        e.set_field(name, value, true);
                    }
                }
                e.touch();
                d.email(&id)?;
            }
            Ok(())
        })?;
    }
    session.lock();
    println!("导入完成；{pending} 个账号待确认归属。原文件保留，机密列已加密存入 KDBX。");
    Ok(())
}
pub fn fill(
    project: &Project,
    identity: &str,
    title: Option<&str>,
    username: Option<&str>,
    generate: bool,
    session: &Session,
) -> Result<()> {
    if title.is_some_and(|t| !t.is_empty()) {
        store::write_password(project, identity, title, username, false, generate, session)?;
        return Ok(());
    }
    let store = Store::open(project, identity, session)?;
    loop {
        let items = store
            .items(identity)?
            .into_iter()
            .filter(|i| i.category == "account")
            .collect::<Vec<_>>();
        let mut choices = items
            .iter()
            .map(|i| {
                format!(
                    "{} · {} · {}",
                    i.key,
                    i.value,
                    if i.password_set {
                        "已设置"
                    } else {
                        "未填密码"
                    }
                )
            })
            .collect::<Vec<_>>();
        choices.push("新增条目".into());
        let Some(i) = ui::choose("选择账号录入密码", &choices)? else {
            return Ok(());
        };
        if i == items.len() {
            store::write_password(project, identity, None, username, false, generate, session)?;
        } else {
            store::write_password(
                project,
                identity,
                Some(&items[i].title()),
                None,
                items[i].password_set,
                generate,
                session,
            )?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn workbook() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "xl/worksheets/sheet1.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        let cell = |col: char, row: usize, text: &str| {
            format!("<c r=\"{col}{row}\" t=\"inlineStr\"><is><t>{text}</t></is></c>")
        };
        let first = HEADERS
            .iter()
            .enumerate()
            .map(|(i, t)| cell((b'A' + i as u8) as char, 1, t))
            .collect::<String>();
        let second = [
            "国内",
            "163",
            "alice",
            "test@example.com",
            "用途",
            "SYNTHETIC-HINT",
            "SYNTHETIC-DETAIL",
        ]
        .iter()
        .enumerate()
        .map(|(i, t)| cell((b'A' + i as u8) as char, 2, t))
        .collect::<String>();
        write!(writer,"<worksheet><sheetData><row r=\"1\">{first}</row><row r=\"2\">{second}</row></sheetData></worksheet>").unwrap();
        writer.finish().unwrap().into_inner()
    }
    #[test]
    fn spreadsheet_only_exposes_public_columns_by_default() {
        let bytes = workbook();
        let rows = read_sheet(&bytes, None).unwrap();
        assert_eq!(rows[0].public[3], "test@example.com");
        assert!(rows[0].secret.is_empty());
        let rows = read_sheet(&bytes, Some(2)).unwrap();
        assert_eq!(&*rows[0].secret[0], "SYNTHETIC-HINT");
    }
}
