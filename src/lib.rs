pub mod ai;
mod ai_view;
pub mod browser;
pub mod clipboard;
pub mod config;
pub mod grants;
pub mod identity;
pub mod import;
mod kdbx;
pub mod maintenance;
pub mod security;
pub mod store;
pub mod ui;
pub mod vault;

use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct Project {
    pub root: PathBuf,
}

impl Project {
    pub fn open(path: &Path) -> Result<Self> {
        let root = path.canonicalize().context("项目目录不存在")?;
        anyhow::ensure!(
            root.join("template").is_dir() && root.join("config").is_dir(),
            "请在 ID 项目目录运行，或使用 --root 指定目录"
        );
        Ok(Self { root })
    }

    pub fn contained(&self, path: &Path) -> Result<()> {
        // 新建路径也核对现有父目录，避免通过软链写进另一个身份。
        anyhow::ensure!(
            path.starts_with(&self.root)
                && !path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
            "路径超出项目目录"
        );
        let mut parent = path;
        while !parent.exists() {
            parent = parent.parent().context("路径无效")?;
        }
        if parent.canonicalize()? != parent {
            bail!("项目数据路径不能使用符号链接");
        }
        Ok(())
    }
}

pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn private_file(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

pub fn valid_name(name: &str) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "身份名只能包含字母、数字、-、_"
    );
    Ok(())
}

pub(crate) fn xml_text(
    event: &quick_xml::events::Event<'_>,
) -> anyhow::Result<Option<zeroize::Zeroizing<String>>> {
    use quick_xml::events::Event;
    let text = match event {
        Event::Text(t) => t.decode()?.into_owned(),
        Event::CData(t) => t.decode()?.into_owned(),
        Event::GeneralRef(r) => match r.resolve_char_ref()? {
            Some(c) => c.to_string(),
            None => quick_xml::escape::resolve_predefined_entity(&r.decode()?)
                .ok_or_else(|| anyhow::anyhow!("不支持自定义 XML 实体"))?
                .into(),
        },
        _ => return Ok(None),
    };
    Ok(Some(zeroize::Zeroizing::new(text)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn data_paths_cannot_escape_or_alias_another_identity() {
        let d = tempfile::tempdir().unwrap();
        let p = Project {
            root: d.path().canonicalize().unwrap(),
        };
        assert!(p.contained(&p.root.join("backups/new")).is_ok());
        assert!(p.contained(&p.root.join("../outside")).is_err());
        #[cfg(unix)]
        {
            fs::create_dir(p.root.join("another-identity")).unwrap();
            std::os::unix::fs::symlink(p.root.join("another-identity"), p.root.join("backups"))
                .unwrap();
            assert!(p.contained(&p.root.join("backups/new")).is_err());
        }
    }
}
