use crate::{
    Project, config, identity, private_dir, private_file, ui, valid_name,
    vault::{Session, run_with},
};
use anyhow::{Context, Result, ensure};
use std::{fs, io::Write, path::Path};

pub fn new_identity(project: &Project, name: &str) -> Result<()> {
    valid_name(name)?;
    ensure!(
        name.bytes()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && name.bytes().all(|c| !c.is_ascii_uppercase()),
        "新身份名须以小写字母或数字开头"
    );
    let target = project.root.join("identities").join(name);
    project.contained(&target)?;
    if target.exists() {
        ensure!(target.canonicalize()? == target, "身份路径不能使用符号链接");
    }
    fn copy_missing(source: &Path, target: &Path) -> Result<()> {
        private_dir(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let path = entry.path();
            let dest = target.join(entry.file_name());
            ensure!(!entry.file_type()?.is_symlink(), "模板不能包含符号链接");
            if path.extension().is_some_and(|v| v == "kdbx") {
                continue;
            }
            if entry.file_type()?.is_dir() {
                ensure!(!dest.is_symlink(), "身份目录包含符号链接");
                copy_missing(&path, &dest)?;
            } else if !dest.exists() {
                let mut out = private_file(&dest)?;
                std::io::copy(&mut fs::File::open(path)?, &mut out)?;
            }
        }
        Ok(())
    }
    copy_missing(&project.root.join("template"), &target)?;
    let vault = target.join("passwords").join(format!("{name}.kdbx"));
    ensure!(
        !vault.parent().unwrap().is_symlink() && !vault.is_symlink(),
        "密码库不能使用符号链接"
    );
    private_dir(vault.parent().unwrap())?;
    if !vault.exists() {
        let initial = config::read_secret(project, "KDBX_INITIAL_PASSWORD")?
            .context("请先在机密配置中设置 KDBX_INITIAL_PASSWORD")?;
        ensure!(!initial.is_empty(), "初始主密码不能为空");
        // run_with 自带一行密码，db-create 需要第二遍确认。
        run_with(&vault, "db-create", &["-p".into()], &initial, &[&initial])?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&vault, fs::Permissions::from_mode(0o600))?;
        }
        println!("已创建身份 {name}。请填写 profile.md，并运行 make passwd id={name} 修改主密码。");
    } else {
        println!("已补齐身份 {name} 的缺失文件，密码库保留。");
    }
    log(project, &format!("identity create-or-backfill {name}"))?;
    Ok(())
}

pub fn log(project: &Project, operation: &str) -> Result<()> {
    let folder = project.root.join("logs");
    project.contained(&folder)?;
    private_dir(&folder)?;
    let path = folder.join("scripts.log");
    project.contained(&path)?;
    let mut options = fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    writeln!(
        options.open(path)?,
        "{} {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        ui::clean(operation)
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn tag(
    project: &Project,
    identity: &str,
    title: &str,
    tags: &str,
    session: &Session,
) -> Result<()> {
    let vault = identity::vault(project, identity)?;
    ensure!(
        !title.chars().any(char::is_control) && !tags.chars().any(char::is_control),
        "标题和标签不能包含控制字符"
    );
    if !ui::confirm(
        "修改标签需要 KeePassXC 导出后重建密码库，会保留加密备份；数据库级自定义设置需复查。继续？",
    )? {
        return Ok(());
    }
    session.unlock(&vault, true)?;
    let xml = session.run(&vault, "export", &[], &[])?;
    let transformed = replace_tags(&xml, title, tags)?;
    // 明文 XML 仅位于匿名内存文件中，不写磁盘或命令参数。
    let fd = rustix::fs::memfd_create("id-tag-xml", rustix::fs::MemfdFlags::CLOEXEC)?;
    use std::os::fd::AsRawFd;
    let mut memory = fs::File::from(fd);
    memory.write_all(&transformed)?;
    let input = format!("/proc/{}/fd/{}", std::process::id(), memory.as_raw_fd());
    let folder = project.root.join("backups/tag-changes").join(identity);
    project.contained(&folder)?;
    private_dir(&folder)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%f").to_string();
    let backup = folder.join(format!("{stamp}.kdbx"));
    let mut file = private_file(&backup)?;
    std::io::copy(&mut fs::File::open(&vault)?, &mut file)?;
    file.sync_all()?;
    let temp = tempfile::Builder::new()
        .prefix(".id-tag-")
        .tempdir_in(vault.parent().unwrap())?;
    let new = temp.path().join("new.kdbx");
    let actual = session.import_xml(&vault, Path::new(&input), &new, title)?;
    fn normalize(s: &str) -> Vec<&str> {
        let mut v = s
            .split([';', ','])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        v.sort_unstable();
        v
    }
    ensure!(
        normalize(std::str::from_utf8(&actual)?) == normalize(tags),
        "标签验证失败，原密码库保留"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&new, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(new, &vault)?;
    session.lock();
    log(project, &format!("tag updated {identity}"))?;
    println!(
        "标签已更新；加密备份：{}",
        backup.strip_prefix(&project.root)?.display()
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn tag(
    _project: &Project,
    _identity: &str,
    _title: &str,
    _tags: &str,
    _session: &Session,
) -> Result<()> {
    anyhow::bail!("此平台请使用 KeePassXC 图形界面修改标签；匿名内存文件方案仅支持 Linux")
}

fn replace_tags(xml: &[u8], title: &str, tags: &str) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    use quick_xml::{
        Reader, Writer,
        events::{BytesEnd, BytesStart, BytesText, Event},
    };
    use zeroize::{Zeroize, Zeroizing};
    // 以完整路径匹配条目；不进入历史条目，拒绝同一路径的重名。
    let mut reader = Reader::from_reader(xml);
    let mut output = Zeroizing::new(Vec::with_capacity(xml.len() + tags.len() * 6 + 128));
    let mut writer = Writer::new(&mut *output);
    let mut groups = Vec::<String>::new();
    let mut group_name = false;
    let mut in_history = 0usize;
    let mut entry_depth = 0;
    let mut entry = Zeroizing::new(Vec::with_capacity(xml.len()));
    let mut key = String::new();
    let mut capture_key = false;
    let mut capture_value = false;
    let mut found = 0;
    let mut entry_title = String::new();
    loop {
        let event = reader.read_event()?;
        if matches!(event, Event::Eof) {
            break;
        }
        if entry_depth == 0 {
            match &event {
                Event::Start(e) if e.name().as_ref() == b"Group" => groups.push(String::new()),
                Event::End(e) if e.name().as_ref() == b"Group" => {
                    groups.pop();
                }
                Event::Start(e) if e.name().as_ref() == b"Name" => group_name = true,
                _ if group_name => {
                    if let (Some(g), Some(text)) = (groups.last_mut(), crate::xml_text(&event)?) {
                        g.push_str(&text);
                    }
                    if matches!(&event,Event::End(e) if e.name().as_ref()==b"Name") {
                        group_name = false;
                    }
                }
                Event::End(e) if e.name().as_ref() == b"Name" => group_name = false,
                _ => {}
            }
        }
        if matches!(&event,Event::Start(e) if e.name().as_ref()==b"Entry") {
            entry_depth += 1;
        }
        if entry_depth > 0 {
            if matches!(&event,Event::Start(e) if e.name().as_ref()==b"History") {
                in_history += 1;
            }
            if in_history == 0 {
                match &event {
                    Event::Start(e) if e.name().as_ref() == b"Key" => {
                        capture_key = true;
                        key.clear();
                    }
                    Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) if capture_key => {
                        key.push_str(&crate::xml_text(&event)?.unwrap())
                    }
                    Event::End(e) if e.name().as_ref() == b"Key" => capture_key = false,
                    Event::Start(e) if e.name().as_ref() == b"Value" && key == "Title" => {
                        capture_value = true;
                        entry_title.clear();
                    }
                    Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) if capture_value => {
                        entry_title.push_str(&crate::xml_text(&event)?.unwrap())
                    }
                    Event::End(e) if e.name().as_ref() == b"Value" => capture_value = false,
                    _ => {}
                }
            }
            {
                let mut buffer = Writer::new(&mut *entry);
                buffer.write_event(event.clone())?;
            }
            if matches!(&event,Event::End(e) if e.name().as_ref()==b"History") {
                in_history = in_history.saturating_sub(1);
            }
            if matches!(&event,Event::End(e) if e.name().as_ref()==b"Entry") {
                entry_depth -= 1;
            }
            if entry_depth == 0 {
                let path = groups
                    .iter()
                    .skip(1)
                    .cloned()
                    .chain(std::iter::once(entry_title.clone()))
                    .collect::<Vec<_>>()
                    .join("/");
                if path == title.trim_start_matches('/') {
                    found += 1;
                    let mut nested = Reader::from_reader(&entry[..]);
                    let mut depth = 0;
                    let mut skip = false;
                    loop {
                        let e = nested.read_event()?;
                        if matches!(e, Event::Eof) {
                            break;
                        }
                        match &e {
                            Event::Start(s) if s.name().as_ref() == b"Entry" => depth += 1,
                            Event::End(s) if s.name().as_ref() == b"Entry" => {
                                depth -= 1;
                                if depth == 0 {
                                    writer.write_event(Event::Start(BytesStart::new("Tags")))?;
                                    writer.write_event(Event::Text(BytesText::new(tags)))?;
                                    writer.write_event(Event::End(BytesEnd::new("Tags")))?;
                                }
                            }
                            Event::Start(s) if s.name().as_ref() == b"Tags" && depth == 1 => {
                                skip = true;
                                continue;
                            }
                            Event::End(s) if s.name().as_ref() == b"Tags" && skip => {
                                skip = false;
                                continue;
                            }
                            Event::Empty(s) if s.name().as_ref() == b"Tags" && depth == 1 => {
                                continue;
                            }
                            _ => {}
                        }
                        if !skip {
                            writer.write_event(e)?;
                        }
                    }
                } else {
                    writer.get_mut().write_all(&entry)?;
                }
                entry.zeroize();
                key.clear();
                entry_title.clear();
            }
        } else {
            writer.write_event(event)?;
        }
    }
    ensure!(found == 1, "未找到唯一的密码条目路径，原库未改动");
    drop(writer);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xml_tag_updates_exact_path_and_preserves_history() {
        let xml=b"<KeePassFile><Root><Group><Name>Root</Name><Entry><String><Key>Title</Key><Value>GitHub</Value></String><Tags>old</Tags><History><Entry><String><Key>Title</Key><Value>Old</Value></String><Tags>historic</Tags></Entry></History></Entry></Group></Root></KeePassFile>";
        let out = replace_tags(xml, "GitHub", "ai-auto").unwrap();
        let text = std::str::from_utf8(&out).unwrap();
        assert!(text.contains("<Tags>ai-auto</Tags>"));
        assert!(text.contains("<Tags>historic</Tags>"));
        assert!(!text.contains("<Tags>old</Tags>"));
        assert!(replace_tags(xml, "wrong", "ai-auto").is_err());
    }
}
