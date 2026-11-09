use crate::{Project, identity, kdbx, private_dir, ui, valid_name, vault::Session};
use anyhow::{Context, Result, ensure};
use std::{fs, io::Write, path::PathBuf};

pub fn new_identity(project: &Project, name: &str) -> Result<()> {
    valid_name(name)?;
    ensure!(
        name.bytes()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && name.bytes().all(|c| !c.is_ascii_uppercase()),
        "新身份名须以小写字母或数字开头"
    );
    let vault = identity::path(project, name)?;
    let password = if !vault.exists() {
        crate::doctor::ensure_no_conflicts(&vault)?;
        let first = ui::secret("设置主密码: ")?;
        let second = ui::secret("再次输入主密码: ")?;
        ensure!(
            !first.is_empty() && first.bytes() == second.bytes(),
            "两次输入不一致，或主密码为空；未创建身份"
        );
        Some(first)
    } else {
        None
    };
    if let Some(password) = password {
        private_dir(vault.parent().unwrap())?;
        kdbx::create(&vault, name, &password)?;
        println!(
            "已创建身份 {name}，基础信息已保存到 ID_info/{name}。用 make manage id={name} 继续填写。"
        );
    } else {
        identity::vault(project, name)?;
        println!("身份 {name} 已存在，密码库与原主密码保留。");
    }
    log(project, &format!("identity create-or-keep {name}"))?;
    Ok(())
}

pub fn remove_identity(
    project: &Project,
    name: &str,
    session: &Session,
) -> Result<Option<PathBuf>> {
    let vault = identity::vault(project, name)?;
    if !ui::confirm(&format!(
        "移除身份 {name}？原文件将归档到本机 backups/removed-identities。"
    ))? {
        return Ok(None);
    }
    session.unlock(&vault, true)?;
    let _lock = kdbx::WriteLock::acquire(project, name)?;
    let source = if vault.parent() == Some(project.root.join("identities").as_path()) {
        vault
    } else {
        project.root.join("identities").join(name)
    };
    project.contained(&source)?;
    let folder = project.root.join("backups/removed-identities");
    project.contained(&folder)?;
    private_dir(&folder)?;
    let archive = tempfile::Builder::new()
        .prefix(&format!("{name}-"))
        .tempdir_in(&folder)?
        .keep();
    private_dir(&archive)?;
    let target = archive.join(source.file_name().context("身份路径无效")?);
    project.contained(&target)?;
    crate::grants::revoke_identity(name)?;
    fs::rename(&source, &target).context("归档失败，身份原文件保留")?;
    session.lock();
    log(project, &format!("identity removed {name}")).context("身份已归档，但操作日志写入失败")?;
    Ok(Some(target))
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

pub fn tag(
    project: &Project,
    identity: &str,
    title: &str,
    tags: &str,
    session: &Session,
) -> Result<()> {
    ensure!(
        !title.chars().any(char::is_control) && !tags.chars().any(char::is_control),
        "标题和标签不能包含控制字符"
    );
    session.unlock(&identity::vault(project, identity)?, true)?;
    crate::store::Store::open(project, identity, session)?.update(|d| {
        let id = d.find_entry(title)?.context("条目不存在")?;
        let e = d.entry_mut(&id)?;
        e.set("Tags", tags);
        e.touch();
        Ok(())
    })?;
    log(project, &format!("tag updated {identity}"))?;
    println!("标签已更新，已保留加密备份。");
    Ok(())
}
