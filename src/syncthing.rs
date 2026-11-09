//! 文件仍由 Syncthing 传输；这里只确认本机与指定交接设备已持有同一版本。
use crate::{Project, config, identity, kdbx, ui, vault::Secret};
use anyhow::{Context, Result, bail, ensure};
use crossterm::{event::KeyCode, event::KeyModifiers};
use reqwest::{Method, blocking::Client, header::HeaderValue};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Api {
    client: Client,
    url: reqwest::Url,
    key: Secret,
}
impl Api {
    fn new(project: &Project, cfg: &config::Syncthing, pending: Option<&Secret>) -> Result<Self> {
        let url = api_url(&cfg.api_url)?;
        let key = if let Some(key) = pending {
            Secret::from_bytes(key.bytes().to_vec())
        } else {
            config::read_secret(project, "SYNCTHING_API_KEY")?
                .context("请在 make settings → Syncthing 同步交接中设置 API key")?
        };
        ensure!(!key.is_empty(), "Syncthing API key 为空");
        Ok(Self {
            client: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(3))
                .build()?,
            url,
            key,
        })
    }
    fn request(&self, method: Method, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let mut url = self.url.clone();
        url.set_path(&format!("{}/rest/{path}", url.path().trim_end_matches('/')));
        let mut key = HeaderValue::from_bytes(self.key.bytes())
            .map_err(|_| anyhow::anyhow!("Syncthing API key 格式无效"))?;
        key.set_sensitive(true);
        let response = self
            .client
            .request(method.clone(), url)
            .query(query)
            .header("X-API-Key", key)
            .send()
            .map_err(|_| anyhow::anyhow!("无法连接 Syncthing，请检查本机接口和服务状态"))?;
        ensure!(
            response.status().is_success(),
            "Syncthing 接口返回 HTTP {}，请检查接口与 API key",
            response.status().as_u16()
        );
        if method == Method::POST {
            return Ok(Value::Null);
        }
        let mut bytes = Vec::new();
        response
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow::anyhow!("读取 Syncthing 状态失败"))?;
        ensure!(bytes.len() <= 1024 * 1024, "Syncthing 状态响应过大");
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("Syncthing 状态响应格式无效"))
    }
    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        self.request(Method::GET, path, query)
    }
}
fn api_url(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value).context("Syncthing 接口地址无效")?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Syncthing 接口地址不能包含凭据、查询参数或片段"
    );
    let local = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    ensure!(
        url.scheme() == "https" || local,
        "远程 Syncthing 接口须使用 HTTPS；本机接口可用 HTTP"
    );
    Ok(url)
}

pub struct Handoff {
    api: Api,
    cfg: config::Syncthing,
    vault: PathBuf,
    file: String,
    original: [u8; 32],
}
impl Handoff {
    pub fn begin(project: &Project, name: &str) -> Result<Option<Self>> {
        let cfg = config::Config::load(project)?.syncthing;
        if !cfg.enabled {
            return Ok(None);
        }
        ensure!(
            !cfg.folder_id.is_empty() && !cfg.peer_device.is_empty(),
            "请在 make settings → Syncthing 同步交接中选择文件夹和交接设备"
        );
        ensure!(
            (1..=300).contains(&cfg.timeout_seconds),
            "同步等待时间须为 1–300 秒"
        );
        let api = Api::new(project, &cfg, None)?;
        let folders = api.get("config/folders", &[])?;
        let folder = folders
            .as_array()
            .and_then(|rows| rows.iter().find(|f| f["id"] == cfg.folder_id))
            .context("Syncthing 中没有配置的同步文件夹")?;
        ensure!(
            folder["paused"] == false && folder["type"] == "sendreceive",
            "同步文件夹须启用且为双向同步"
        );
        ensure!(
            folder["devices"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|d| d["deviceID"] == cfg.peer_device)),
            "交接设备未共享此同步文件夹"
        );
        let path = folder["path"].as_str().context("同步文件夹路径缺失")?;
        let path = if let Some(relative) = path.strip_prefix("~/") {
            PathBuf::from(std::env::var_os("HOME").context("无法展开同步文件夹路径")?)
                .join(relative)
        } else {
            PathBuf::from(path)
        };
        ensure!(path.is_absolute(), "Syncthing 同步文件夹须使用绝对路径");
        let root = path
            .canonicalize()
            .context("无法访问本机同步文件夹；接口须属于当前设备的 Syncthing")?;
        let vault = identity::vault(project, name)?;
        let file = vault
            .strip_prefix(&root)
            .context("密码库不在所选本机同步文件夹中")?
            .to_str()
            .context("密码库路径须为 UTF-8")?
            .replace(std::path::MAIN_SEPARATOR, "/");
        let mut handoff = Self {
            api,
            cfg,
            original: kdbx::fingerprint(&vault)?,
            vault,
            file,
        };
        handoff.wait("打开前：等待本机与交接设备同步完成")?;
        handoff.original = kdbx::fingerprint(&handoff.vault)?;
        Ok(Some(handoff))
    }
    pub fn finish(self) -> Result<()> {
        self.conflicts()?;
        if kdbx::fingerprint(&self.vault)? == self.original {
            return Ok(());
        }
        self.wait("退出前：等待交接设备收到更新")?;
        println!("已确认交接设备收到更新，可以切换设备。");
        Ok(())
    }
    fn conflicts(&self) -> Result<()> {
        let stem = self.vault.file_stem().unwrap().to_string_lossy();
        for item in fs::read_dir(self.vault.parent().unwrap())? {
            let name = item?.file_name().to_string_lossy().into_owned();
            ensure!(
                !(name.starts_with(&format!("{stem}.sync-conflict-")) && name.ends_with(".kdbx")),
                "存在此身份的同步冲突副本，请在 KeePassXC 合并并核对后继续；所有文件均保留"
            );
        }
        Ok(())
    }
    fn ready(&self) -> Result<bool> {
        self.conflicts()?;
        let query = [("folder", self.cfg.folder_id.as_str())];
        let connections = self.api.get("system/connections", &[])?;
        let peer = &connections["connections"][&self.cfg.peer_device];
        ensure!(
            peer["connected"] == true && peer["paused"] != true,
            "交接设备未连接或已暂停，尚不能确认同步完成"
        );
        let state = self.api.get("db/status", &query)?;
        ensure!(
            state["pullErrors"].as_u64() == Some(0),
            "同步存在错误，请在 Syncthing 查看失败原因"
        );
        if state["state"] != "idle" || state["needTotalItems"].as_u64() != Some(0) {
            return Ok(false);
        }
        let completion = self.api.get(
            "db/completion",
            &[
                ("folder", &self.cfg.folder_id),
                ("device", &self.cfg.peer_device),
            ],
        )?;
        if completion["remoteState"] != "valid"
            || ["needItems", "needBytes", "needDeletes"]
                .iter()
                .any(|k| completion[k].as_u64() != Some(0))
        {
            return Ok(false);
        }
        let file = self.api.get(
            "db/file",
            &[("folder", &self.cfg.folder_id), ("file", &self.file)],
        )?;
        Ok(same_version(
            &file,
            &self.cfg.peer_device,
            fs::metadata(&self.vault)?.len(),
        ))
    }
    fn wait(&self, title: &str) -> Result<()> {
        let _terminal = ui::Terminal::new(true)?;
        loop {
            let result = (|| -> Result<()> {
                self.conflicts()?;
                let mut before = kdbx::fingerprint(&self.vault)?;
                ui::draw(&[
                    ui::color(title, "1;36"),
                    "检查同步状态…".into(),
                    "Esc / Ctrl+C 取消".into(),
                ])?;
                self.api.request(
                    Method::POST,
                    "db/scan",
                    &[("folder", &self.cfg.folder_id), ("sub", &self.file)],
                )?;
                let start = Instant::now();
                loop {
                    let mut ready = self.ready()?;
                    let current = kdbx::fingerprint(&self.vault)?;
                    if current != before {
                        before = current;
                        self.api.request(
                            Method::POST,
                            "db/scan",
                            &[("folder", &self.cfg.folder_id), ("sub", &self.file)],
                        )?;
                        ready = false;
                    }
                    if ready {
                        return Ok(());
                    }
                    ensure!(
                        start.elapsed() < Duration::from_secs(self.cfg.timeout_seconds),
                        "等待同步超时，尚未确认交接设备收到最新版本"
                    );
                    ui::draw(&[
                        ui::color(title, "1;36"),
                        format!("等待同步 · {} 秒", start.elapsed().as_secs()),
                        "Esc / Ctrl+C 取消".into(),
                    ])?;
                    if let Some(key) = ui::key(Duration::from_millis(500))?
                        && (key.code == KeyCode::Esc
                            || key.code == KeyCode::Char('q')
                            || key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        bail!("已取消");
                    }
                }
            })();
            match result {
                Ok(()) => return Ok(()),
                Err(error) if error.to_string() == "已取消" => {
                    bail!("同步未确认完成，请勿切换设备")
                }
                Err(error) => {
                    let message = format!("{}\n同步未确认完成", error);
                    if ui::choose(
                        &message,
                        &[
                            "重试同步检查".into(),
                            "退出（未确认同步，请勿切换设备）".into(),
                        ],
                    )? != Some(0)
                    {
                        bail!("同步未确认完成，请勿切换设备：{error}");
                    }
                }
            }
        }
    }
}

// shortcut: 依赖 Syncthing 已公布的索引；需要强制跨客户端互斥时须使用中心租约。
fn same_version(file: &Value, peer: &str, size: u64) -> bool {
    let local = &file["local"];
    let global = &file["global"];
    let version = |entry: &Value| {
        entry["version"]
            .as_array()
            .filter(|v| !v.is_empty())
            .and_then(|v| {
                v.iter()
                    .map(|s| s.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
            })
            .map(|mut v| {
                v.sort();
                v
            })
    };
    version(local).is_some()
        && version(local) == version(global)
        && [local, global].iter().all(|e| {
            e["size"].as_u64() == Some(size)
                && e["deleted"] == false
                && e["invalid"] == false
                && e["ignored"] != true
                && e["mustRescan"] != true
        })
        && file["availability"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|r| r["id"] == peer && r["fromTemporary"] == false)
        })
}

pub fn settings(
    project: &Project,
    cfg: &mut config::Syncthing,
    key: &mut Option<Secret>,
) -> Result<()> {
    let mut selected = 0;
    loop {
        let rows = vec![
            format!(
                "同步交接        {}",
                if cfg.enabled {
                    "● 开  ○ 关"
                } else {
                    "○ 开  ● 关"
                }
            ),
            format!("本机接口        {}", cfg.api_url),
            format!(
                "API key         {}",
                if key.is_some()
                    || config::read_secret(project, "SYNCTHING_API_KEY")?
                        .is_some_and(|v| !v.is_empty())
                {
                    "已设置"
                } else {
                    "未设置"
                }
            ),
            format!(
                "同步文件夹      {}",
                if cfg.folder_id.is_empty() {
                    "未选择"
                } else {
                    &cfg.folder_id
                }
            ),
            format!(
                "交接设备 / NAS  {}",
                if cfg.peer_device.is_empty() {
                    "未选择"
                } else {
                    &cfg.peer_device
                }
            ),
            "返回（随主设置保存）".into(),
        ];
        let choice = ui::choose_from(
            "Syncthing 同步交接 · 连接当前设备的 Syncthing",
            &rows,
            selected,
        )?;
        if let Some(index) = choice {
            selected = index;
        }
        match choice {
            Some(0) => cfg.enabled = !cfg.enabled,
            Some(1) => {
                let value = ui::input("本机 Syncthing 接口（Enter 保留）: ")?;
                if !value.is_empty() {
                    api_url(&value)?;
                    cfg.api_url = value;
                }
            }
            Some(2) => {
                let value = ui::secret("Syncthing API key（Enter 保留）: ")?;
                if !value.is_empty() {
                    *key = Some(value);
                }
            }
            Some(3) | Some(4) => {
                let api = Api::new(project, cfg, key.as_ref())?;
                let folders = choice == Some(3);
                let data = api.get(
                    if folders {
                        "config/folders"
                    } else {
                        "config/devices"
                    },
                    &[],
                )?;
                let own_id = if folders {
                    Value::Null
                } else {
                    api.get("system/status", &[])?["myID"].clone()
                };
                let items = data
                    .as_array()
                    .context("Syncthing 配置响应无效")?
                    .iter()
                    .filter(|v| folders || v["deviceID"] != own_id)
                    .collect::<Vec<_>>();
                let labels = items
                    .iter()
                    .map(|v| {
                        if folders {
                            format!(
                                "{} · {} · {}",
                                v["label"].as_str().unwrap_or(""),
                                v["id"].as_str().unwrap_or(""),
                                v["path"].as_str().unwrap_or("")
                            )
                        } else {
                            format!(
                                "{} · {}",
                                v["name"].as_str().unwrap_or(""),
                                v["deviceID"].as_str().unwrap_or("")
                            )
                        }
                    })
                    .collect::<Vec<_>>();
                ensure!(!labels.is_empty(), "没有可选的同步文件夹或其他设备");
                if let Some(index) = ui::choose(
                    if folders {
                        "选择包含 identities 的本机同步文件夹"
                    } else {
                        "选择交接设备（通常为 NAS）"
                    },
                    &labels,
                )? {
                    let value = items[index][if folders { "id" } else { "deviceID" }]
                        .as_str()
                        .context("Syncthing 配置标识缺失")?
                        .to_owned();
                    if folders {
                        cfg.folder_id = value;
                    } else {
                        cfg.peer_device = value;
                    }
                }
            }
            _ => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn peer_must_hold_complete_matching_version() {
        let entry = json!({"size":12,"version":["A:1","B:2"],"deleted":false,"invalid":false});
        let mut file = json!({"local":entry,"global":entry,"availability":[{"id":"NAS","fromTemporary":false}]});
        assert!(same_version(&file, "NAS", 12));
        file["global"]["version"] = json!(["B:2", "A:1"]);
        assert!(same_version(&file, "NAS", 12));
        file["availability"][0]["fromTemporary"] = json!(true);
        assert!(!same_version(&file, "NAS", 12));
        file["availability"][0]["fromTemporary"] = json!(false);
        assert!(!same_version(&file, "OTHER", 12));
        file["global"]["version"] = json!(["A:2"]);
        assert!(!same_version(&file, "NAS", 12));
        assert!(!same_version(&json!({}), "NAS", 0));
    }
    #[test]
    fn credentials_are_not_sent_to_plaintext_remote_or_url_credentials() {
        assert!(api_url("http://127.0.0.1:8384").is_ok());
        assert!(api_url("http://[::1]:8384").is_ok());
        assert!(api_url("https://nas.example:8384").is_ok());
        assert!(api_url("http://nas.example:8384").is_err());
        assert!(api_url("https://key@nas.example").is_err());
        assert!(api_url("http://localhost:8384?apikey=value").is_err());
    }
}
