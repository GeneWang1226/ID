use crate::{Project, identity, kdbx, private_dir, private_file, ui, valid_name, vault::Session};
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
    let vault = target.join("passwords").join(format!("{name}.kdbx"));
    project.contained(&vault)?;
    ensure!(
        !vault.parent().unwrap().is_symlink() && !vault.is_symlink(),
        "密码库不能使用符号链接"
    );
    let password = if !vault.exists() {
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
    fn copy_missing(source: &Path, target: &Path) -> Result<()> {
        private_dir(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let path = entry.path();
            let dest = target.join(entry.file_name());
            ensure!(!entry.file_type()?.is_symlink(), "模板不能包含符号链接");
            if entry.file_name() == "data" || path.extension().is_some_and(|v| v == "kdbx") {
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
    ensure!(
        !vault.parent().unwrap().is_symlink() && !vault.is_symlink(),
        "密码库不能使用符号链接"
    );
    private_dir(vault.parent().unwrap())?;
    if let Some(password) = password {
        kdbx::create(&vault, name, &password)?;
        println!(
            "已创建身份 {name}，基础信息已保存到 ID_info/{name}。用 make manage id={name} 继续填写。"
        );
    } else {
        println!("已补齐身份 {name} 的缺失文件；已有密码库与原主密码保留。");
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
