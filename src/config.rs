use crate::{Project, private_dir, ui, vault::Secret};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write};
use zeroize::Zeroizing;

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Ai {
    pub base_url: String,
    pub model: String,
    pub thinking: bool,
    pub thinking_budget: u32,
}
impl Default for Ai {
    fn default() -> Self {
        Self {
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            model: "qwen-plus".into(),
            thinking: false,
            thinking_budget: 8192,
        }
    }
}
#[derive(Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Terminal {
    pub allow_plaintext: bool,
}
#[derive(Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub ai: Ai,
    pub terminal: Terminal,
}

impl Config {
    pub fn load(project: &Project) -> Result<Self> {
        let path = project.root.join("config/config.toml");
        project.contained(&path)?;
        if !path.exists() {
            return Ok(Self::default());
        }
        toml::from_str(&fs::read_to_string(path)?).context("config/config.toml 格式无效")
    }
    pub fn plaintext_allowed(project: &Project) -> bool {
        Self::load(project).is_ok_and(|c| c.terminal.allow_plaintext)
    }
    pub fn save(&self, project: &Project) -> Result<()> {
        let path = project.root.join("config/config.toml");
        project.contained(&path)?;
        fs::create_dir_all(path.parent().unwrap())?;
        let mut value: toml::Value = if path.exists() {
            toml::from_str(&fs::read_to_string(&path)?)?
        } else {
            toml::Value::Table(Default::default())
        };
        let replacement = toml::Value::try_from(self)?;
        for name in ["ai", "terminal"] {
            let table = value.as_table_mut().context("配置根节点须为表")?;
            let section = table
                .entry(name)
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .context("配置分组须为表")?;
            for (k, v) in replacement[name].as_table().unwrap() {
                section.insert(k.clone(), v.clone());
            }
        }
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        file.write_all(toml::to_string_pretty(&value)?.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(&path)
            .map_err(|_| anyhow::anyhow!("保存配置失败"))?;
        Ok(())
    }
}

pub fn read_secret(project: &Project, key: &str) -> Result<Option<Secret>> {
    let path = project.root.join("config/secrets/.secrets.env");
    project.contained(&path)?;
    if !path.exists() {
        return Ok(None);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            fs::metadata(&path)?.permissions().mode() & 0o077 == 0,
            "机密配置权限必须为 600"
        );
    }
    let bytes = Zeroizing::new(fs::read(path)?);
    let text = std::str::from_utf8(&bytes).context("机密配置必须是 UTF-8")?;
    for line in text.lines() {
        if let Some((k, v)) = line.trim().split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                let v = if v.len() >= 2
                    && (v.starts_with('"') && v.ends_with('"')
                        || v.starts_with('\'') && v.ends_with('\''))
                {
                    &v[1..v.len() - 1]
                } else {
                    v
                };
                return Ok(Some(Secret::from_bytes(v.as_bytes().to_vec())));
            }
        }
    }
    Ok(None)
}

pub fn update_secret(project: &Project, key: &str, value: &Secret) -> Result<()> {
    ensure!(
        key.bytes().all(|c| c.is_ascii_uppercase() || c == b'_'),
        "机密键名无效"
    );
    ensure!(
        !value.bytes().contains(&b'\n') && !value.bytes().contains(&b'\r'),
        "机密值不能包含换行"
    );
    let path = project.root.join("config/secrets/.secrets.env");
    project.contained(&path)?;
    private_dir(path.parent().unwrap())?;
    let previous = Zeroizing::new(if path.exists() {
        fs::read(&path)?
    } else {
        Vec::new()
    });
    let text = std::str::from_utf8(&previous)?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    for line in text.lines() {
        if line
            .trim()
            .split_once('=')
            .is_none_or(|(k, _)| k.trim() != key)
        {
            writeln!(file, "{line}")?;
        }
    }
    file.write_all(key.as_bytes())?;
    file.write_all(b"=")?;
    file.write_all(value.bytes())?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|_| anyhow::anyhow!("保存机密配置失败"))?;
    Ok(())
}

pub fn settings(project: &Project) -> Result<()> {
    let mut cfg = Config::load(project)?;
    let mut pending = None;
    let mut selected = 0;
    loop {
        let key_set =
            pending.is_some() || read_secret(project, "AI_API_KEY")?.is_some_and(|v| !v.is_empty());
        let toggle = |b| {
            if b {
                "● 开  ○ 关"
            } else {
                "○ 开  ● 关"
            }
        };
        let rows = vec![
            format!(
                "AI API key       {}",
                if key_set { "已设置" } else { "未设置" }
            ),
            format!("AI 接口地址      {}", cfg.ai.base_url),
            format!("AI 模型          {}", cfg.ai.model),
            format!("思考模式         {}", toggle(cfg.ai.thinking)),
            format!("思考预算         {} token", cfg.ai.thinking_budget),
            format!("明文显示密码     {}", toggle(cfg.terminal.allow_plaintext)),
            "保存并退出".into(),
            "取消".into(),
        ];
        let choice = ui::choose_from("ID 设置", &rows, selected)?;
        if let Some(i) = choice {
            selected = i;
        }
        match choice {
            Some(0) => {
                let value = ui::secret("新的 AI API key（Enter 保留）: ")?;
                if !value.is_empty() {
                    pending = Some(value);
                }
            }
            Some(1) => {
                let v = ui::input("AI 接口地址（Enter 保留）: ")?;
                if !v.is_empty() {
                    let url = reqwest::Url::parse(&v).context("接口地址无效")?;
                    ensure!(
                        matches!(url.scheme(), "http" | "https")
                            && url.username().is_empty()
                            && url.password().is_none(),
                        "接口地址不能包含凭据"
                    );
                    cfg.ai.base_url = v;
                }
            }
            Some(2) => {
                let v = ui::input("AI 模型（Enter 保留）: ")?;
                if !v.is_empty() {
                    cfg.ai.model = v;
                }
            }
            Some(3) => cfg.ai.thinking = !cfg.ai.thinking,
            Some(4) => {
                let v = ui::input("思考预算（Enter 保留）: ")?;
                if !v.is_empty() {
                    let budget = v.parse::<u32>().context("思考预算须为正整数")?;
                    ensure!(budget > 0, "思考预算须为正整数");
                    cfg.ai.thinking_budget = budget;
                }
            }
            Some(5) => cfg.terminal.allow_plaintext = !cfg.terminal.allow_plaintext,
            Some(6) => {
                if let Some(v) = pending {
                    update_secret(project, "AI_API_KEY", &v)?;
                }
                cfg.save(project)?;
                println!("设置已保存。");
                return Ok(());
            }
            _ => {
                println!("已取消。");
                return Ok(());
            }
        }
    }
}

pub fn ensure_plaintext(project: &Project) -> Result<()> {
    ensure!(
        Config::plaintext_allowed(project),
        "明文显示已禁用，可在设置中修改"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project(path: &std::path::Path) -> Project {
        Project {
            root: path.canonicalize().unwrap(),
        }
    }
    #[test]
    fn settings_preserve_unknown_values_and_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path());
        fs::create_dir(dir.path().join("config")).unwrap();
        fs::write(dir.path().join("config/config.toml"), "[extra]\nkeep=42\n").unwrap();
        let mut c = Config::load(&p).unwrap();
        assert!(!c.terminal.allow_plaintext);
        c.terminal.allow_plaintext = true;
        c.save(&p).unwrap();
        assert!(Config::plaintext_allowed(&p));
        assert!(
            fs::read_to_string(dir.path().join("config/config.toml"))
                .unwrap()
                .contains("keep = 42")
        );
        fs::write(dir.path().join("config/config.toml"), "bad!").unwrap();
        assert!(!Config::plaintext_allowed(&p));
    }
    #[test]
    fn secret_update_keeps_other_keys_and_rejects_newlines() {
        let d = tempfile::tempdir().unwrap();
        let p = project(d.path());
        update_secret(
            &p,
            "OTHER",
            &Secret::from_bytes(b"synthetic-other".to_vec()),
        )
        .unwrap();
        update_secret(
            &p,
            "AI_API_KEY",
            &Secret::from_bytes(b"synthetic-key".to_vec()),
        )
        .unwrap();
        assert_eq!(
            read_secret(&p, "OTHER").unwrap().unwrap().bytes(),
            b"synthetic-other"
        );
        assert!(update_secret(&p, "AI_API_KEY", &Secret::from_bytes(b"x\ny".to_vec())).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(d.path().join("config/secrets/.secrets.env"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
