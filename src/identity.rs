use crate::{Project, ui, valid_name};
use anyhow::{Result, ensure};
use std::{fs, path::PathBuf};

pub fn names(project: &Project) -> Result<Vec<String>> {
    let folder = project.root.join("identities");
    if !folder.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        let name = if entry.file_type()?.is_file() && path.extension().is_some_and(|s| s == "kdbx")
        {
            path.file_stem().unwrap().to_string_lossy().into_owned()
        } else if entry.file_type()?.is_dir() {
            entry.file_name().to_string_lossy().into_owned()
        } else {
            continue;
        };
        if valid_name(&name).is_ok()
            && !name.contains(".sync-conflict-")
            && (path.is_file()
                || path
                    .join("passwords")
                    .join(format!("{name}.kdbx"))
                    .is_file())
        {
            result.push(name);
        }
    }
    result.sort();
    result.dedup();
    Ok(result)
}
pub fn select(project: &Project, name: Option<&str>) -> Result<String> {
    let name = if let Some(name) = name.filter(|v| !v.is_empty()) {
        name.to_owned()
    } else {
        let names = names(project)?;
        ensure!(!names.is_empty(), "尚未创建身份");
        names[ui::choose("选择身份", &names)?.ok_or_else(|| anyhow::anyhow!("已取消"))?].clone()
    };
    valid_name(&name)?;
    vault(project, &name)?;
    Ok(name)
}
pub(crate) fn path(project: &Project, name: &str) -> Result<PathBuf> {
    valid_name(name)?;
    let folder = project.root.join("identities");
    let path = folder.join(format!("{name}.kdbx"));
    let legacy = folder
        .join(name)
        .join("passwords")
        .join(format!("{name}.kdbx"));
    project.contained(&path)?;
    project.contained(&legacy)?;
    ensure!(
        !path.is_symlink() && !legacy.is_symlink(),
        "密码库不能使用符号链接"
    );
    ensure!(
        !(path.exists() && legacy.exists()),
        "同一身份存在新旧两个密码库，请先核对并归档旧目录"
    );
    Ok(if legacy.exists() { legacy } else { path })
}
pub fn vault(project: &Project, name: &str) -> Result<PathBuf> {
    let path = path(project, name)?;
    ensure!(
        path.is_file() && path.canonicalize()? == path,
        "密码库不存在，或路径使用了符号链接"
    );
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_identity_traversal_and_aliases() {
        let d = tempfile::tempdir().unwrap();
        let p = Project {
            root: d.path().canonicalize().unwrap(),
        };
        fs::create_dir_all(p.root.join("identities")).unwrap();
        fs::write(
            p.root.join("identities/alice.kdbx"),
            b"synthetic opaque fixture",
        )
        .unwrap();
        assert_eq!(select(&p, Some("alice")).unwrap(), "alice");
        fs::write(
            p.root.join("identities/alice.sync-conflict-test.kdbx"),
            b"synthetic conflict fixture",
        )
        .unwrap();
        assert_eq!(names(&p).unwrap(), ["alice"]);
        assert!(select(&p, Some("../bob")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                p.root.join("identities/alice.kdbx"),
                p.root.join("identities/bob.kdbx"),
            )
            .unwrap();
            assert!(select(&p, Some("bob")).is_err());
        }
        fs::create_dir_all(p.root.join("identities/alice/passwords")).unwrap();
        fs::write(
            p.root.join("identities/alice/passwords/alice.kdbx"),
            b"synthetic legacy fixture",
        )
        .unwrap();
        assert!(vault(&p, "alice").is_err());
        assert_eq!(names(&p).unwrap(), ["alice"]);
        fs::remove_file(p.root.join("identities/alice.kdbx")).unwrap();
        assert_eq!(select(&p, Some("alice")).unwrap(), "alice");
        assert!(
            vault(&p, "alice")
                .unwrap()
                .ends_with("alice/passwords/alice.kdbx")
        );
    }
}
