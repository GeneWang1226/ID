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
        let name = entry.file_name().to_string_lossy().into_owned();
        if valid_name(&name).is_ok() && entry.file_type()?.is_dir() {
            result.push(name);
        }
    }
    result.sort();
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
    let path = project.root.join("identities").join(&name);
    ensure!(
        path.is_dir() && path.canonicalize()? == path,
        "身份不存在，或身份目录使用了符号链接"
    );
    Ok(name)
}
pub fn vault(project: &Project, name: &str) -> Result<PathBuf> {
    select(project, Some(name))?;
    let path = project
        .root
        .join("identities")
        .join(name)
        .join("passwords")
        .join(format!("{name}.kdbx"));
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
        fs::create_dir_all(p.root.join("identities/alice")).unwrap();
        assert_eq!(select(&p, Some("alice")).unwrap(), "alice");
        assert!(select(&p, Some("../bob")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                p.root.join("identities/alice"),
                p.root.join("identities/bob"),
            )
            .unwrap();
            assert!(select(&p, Some("bob")).is_err());
        }
    }
}
