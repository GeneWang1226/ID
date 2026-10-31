use crate::{
    Project,
    config::{self, Config},
    store::{self, Store},
    ui,
    vault::Session,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

fn prompt(identity: &str) -> String {
    format!(
        r#"你是个人资料整理助手。当前身份为 {identity}，所有输出必须归属该身份。
只输出 JSON 对象：{{"reply":"简短中文说明","items":[{{"identity":"{identity}","category":"account/work/address 等维度","key":"名称","value":"内容","note":"备注"}}],"events":[{{"identity":"{identity}","title":"事件","detail":"说明"}}],"secrets":[{{"identity":"{identity}","title":"本地密码条目标题","kind":"密码","action":"copy/save"}}]}}。
只提取有实际内容的资料，不生成空值占位条目，不编造记录。
密码、密钥、恢复码和授权码绝不能输出或写入 items/events。查看已有密码时只输出 secrets 的标题和 action=copy；保存或重置时只输出标题和 action=save，修改已有密码时 action=edit，让用户在本地终端输入密码。禁止要求用户向聊天提供密码明文。
资料不足时返回空数组并解释。"#
    )
}

pub fn call(project: &Project, identity: &str, text: &str, history: &[Value]) -> Result<Value> {
    let cfg = Config::load(project)?;
    let api_key = config::read_secret(project, "AI_API_KEY")?
        .or_else(|| {
            std::env::var("AI_API_KEY")
                .ok()
                .map(|s| crate::vault::Secret::from_bytes(s.into_bytes()))
        })
        .context("请在 settings 中设置 AI API key")?;
    let base = std::env::var("AI_BASE_URL").unwrap_or(cfg.ai.base_url);
    let url = reqwest::Url::parse(&(base.trim_end_matches('/').to_owned() + "/chat/completions"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none(),
        "AI 接口地址不能包含凭据"
    );
    let model = std::env::var("AI_MODEL").unwrap_or(cfg.ai.model);
    let mut messages = vec![json!({"role":"system","content":prompt(identity)})];
    messages.extend(
        history
            .iter()
            .rev()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev(),
    );
    messages.push(json!({"role":"user","content":text}));
    let mut body = json!({"model":model,"messages":messages,"response_format":{"type":"json_object"},"temperature":0.1,"enable_thinking":cfg.ai.thinking});
    if cfg.ai.thinking && cfg.ai.thinking_budget > 0 {
        body["thinking_budget"] = json!(cfg.ai.thinking_budget);
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .post(url)
        .bearer_auth(std::str::from_utf8(api_key.bytes())?)
        .json(&body)
        .send()
        .map_err(|_| anyhow::anyhow!("AI 接口连接失败"))?;
    ensure!(
        response.status().is_success(),
        "AI 接口返回 HTTP {}",
        response.status()
    );
    let data: Value = response.json().context("AI 响应格式无效")?;
    let message = &data["choices"][0]["message"];
    if let Some(reasoning) = message["reasoning_content"]
        .as_str()
        .filter(|s| !s.is_empty())
    {
        println!(
            "{}",
            ui::color(&format!("思考 › {}", ui::clean(reasoning)), "2")
        );
    }
    let content = message["content"].as_str().context("AI 未返回文本")?;
    let first = content.find('{').context("AI 未返回 JSON 对象")?;
    let last = content.rfind('}').context("AI 未返回 JSON 对象")?;
    ensure!(first <= last, "AI 返回的 JSON 无效");
    let draft: Value =
        serde_json::from_str(&content[first..=last]).context("AI 返回的 JSON 无效")?;
    ensure!(draft.is_object(), "AI 返回的 JSON 不是对象");
    Ok(draft)
}

fn rows(draft: &Value, key: &str, required: &[&str]) -> Vec<Value> {
    draft[key]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|r| {
                    r.is_object()
                        && required
                            .iter()
                            .all(|k| r[k].as_str().is_some_and(|s| !s.trim().is_empty()))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}
fn local_secret(text: &str) -> Option<&'static str> {
    let text = text.to_lowercase();
    if ![
        "密码",
        "授权码",
        "助记词",
        "密钥",
        "password",
        "passwd",
        "token",
        "secret",
    ]
    .iter()
    .any(|s| text.contains(s))
    {
        return None;
    }
    if ["重新设置", "重置", "修改", "更新", "reset", "update"]
        .iter()
        .any(|s| text.contains(s))
    {
        return Some("edit");
    }
    if [
        "保存", "存入", "录入", "新增", "添加", "设置", "重置", "修改", "更新", "save", "add",
        "store", "reset", "update",
    ]
    .iter()
    .any(|s| text.contains(s))
    {
        Some("save")
    } else if [
        "查看",
        "看看",
        "查询",
        "查一下",
        "看",
        "显示",
        "复制",
        "取出",
        "使用",
        "view",
        "show",
        "copy",
        "retrieve",
    ]
    .iter()
    .any(|s| text.contains(s))
    {
        Some("copy")
    } else {
        None
    }
}
pub fn apply(store: &mut Store, identity: &str, draft: &Value) -> Result<()> {
    let items = rows(draft, "items", &["category", "key", "value"]);
    let events = rows(draft, "events", &["title"]);
    for row in items.iter().chain(&events) {
        ensure!(
            row.get("identity")
                .is_none_or(|v| v.as_str().is_some_and(|s| s.is_empty() || s == identity)),
            "草稿包含其他身份，未写入"
        );
    }
    let tx = store.db.transaction()?;
    for row in items {
        tx.execute("INSERT INTO items(identity,category,key,value,note) VALUES(?,?,?,?,?) ON CONFLICT(identity,category,key) DO UPDATE SET value=excluded.value,note=excluded.note,updated_at=datetime('now')",rusqlite::params![identity,row["category"].as_str().unwrap(),row["key"].as_str().unwrap(),row["value"].as_str().unwrap(),row["note"].as_str().unwrap_or("")])?;
    }
    for row in events {
        tx.execute(
            "INSERT INTO events(identity,title,detail) VALUES(?,?,?)",
            rusqlite::params![
                identity,
                row["title"].as_str().unwrap(),
                row["detail"].as_str().unwrap_or("")
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn chat(
    project: &Project,
    identity: &str,
    file: Option<&Path>,
    session: &Session,
) -> Result<()> {
    ui::require_terminal()?;
    let mut first = if let Some(file) = file {
        let path = file.canonicalize()?;
        ensure!(
            path.starts_with(&project.root)
                && !["identities", "config/secrets", "backups", "logs", ".git"]
                    .iter()
                    .any(|folder| path.starts_with(project.root.join(folder))),
            "仅接受项目内明确提供的非机密资料文件，不读取身份、秘密配置或备份目录"
        );
        Some(fs::read_to_string(path)?)
    } else {
        None
    };
    let mut history = Vec::new();
    let mut store = Store::open(project)?;
    if let Ok(vault) = crate::identity::vault(project, identity) {
        session.unlock(&vault, false)?;
    }
    let _ = crate::clipboard::check(project, false)?;
    loop {
        let text = if let Some(text) = first.take() {
            text
        } else {
            println!(
                "\n{}",
                ui::color(
                    &format!("AI 资料整理 / {identity} · 空行提交，/lock 锁定，/quit 退出"),
                    "1;36"
                )
            );
            let mut lines = Vec::new();
            loop {
                let line = ui::input(if lines.is_empty() {
                    "你 › "
                } else {
                    "  │ "
                })?;
                if line.is_empty() {
                    break;
                }
                lines.push(line);
            }
            lines.join("\n")
        };
        if text == "/quit" {
            return Ok(());
        }
        if text == "/lock" {
            session.lock();
            println!("密码库已锁定。");
            continue;
        }
        if text.trim().is_empty() {
            continue;
        }
        println!("{}", ui::color("AI › 正在整理…", "2"));
        let draft = call(project, identity, &text, &history)?;
        history.push(json!({"role":"user","content":text}));
        history.push(json!({"role":"assistant","content":draft.to_string()}));
        if history.len() > 10 {
            history.drain(..history.len() - 10);
        }
        println!(
            "{}",
            ui::color(
                &format!("AI › {}", ui::clean(draft["reply"].as_str().unwrap_or(""))),
                "2"
            )
        );
        println!(
            "{}",
            ui::color(&ui::clean(&serde_json::to_string_pretty(&draft)?), "2")
        );
        let items = rows(&draft, "items", &["category", "key", "value"]);
        let events = rows(&draft, "events", &["title"]);
        for item in &items {
            println!(
                "{} / {} = {}",
                ui::clean(item["category"].as_str().unwrap()),
                ui::clean(item["key"].as_str().unwrap()),
                ui::clean(item["value"].as_str().unwrap())
            );
        }
        for event in &events {
            println!("事件：{}", ui::clean(event["title"].as_str().unwrap()));
        }
        let mut secrets = rows(&draft, "secrets", &["title"]);
        if secrets.is_empty() {
            if let Some(action) = local_secret(&text) {
                secrets.push(json!({"title":store::infer_title(&text),"action":action}));
            }
        }
        let mut choices = Vec::new();
        let mut actions = Vec::new();
        if !items.is_empty() || !events.is_empty() {
            choices.push("确认写入资料库".into());
            actions.push(("write", String::new()));
        }
        for secret in &secrets {
            let title = secret["title"].as_str().unwrap();
            let action = local_secret(&text).unwrap_or(if secret["action"] == "copy" {
                "copy"
            } else {
                "save"
            });
            let target = match action {
                "copy" => "pass",
                "edit" => "pass-edit",
                _ => "pass-add-manual",
            };
            println!(
                "{}",
                ui::color(
                    &ui::clean(&format!(
                        "本地操作：make {target} id={} title={}",
                        shell_quote(identity),
                        shell_quote(title)
                    )),
                    "33"
                )
            );
            choices.push(format!(
                "{} {}",
                if action == "copy" {
                    "复制密码"
                } else if action == "edit" {
                    "修改密码"
                } else {
                    "录入密码"
                },
                ui::clean(title)
            ));
            actions.push((action, title.into()));
            if action == "copy" && Config::plaintext_allowed(project) {
                choices.push(format!("明文查看 {}", ui::clean(title)));
                actions.push(("show", title.into()));
            }
        }
        choices.extend(["继续向 AI 提问".into(), "退出".into()]);
        actions.extend([("continue", String::new()), ("exit", String::new())]);
        let Some(index) = ui::choose("下一步", &choices)? else {
            return Ok(());
        };
        let (action, title) = &actions[index];
        match *action {
            "write" => {
                apply(&mut store, identity, &draft)?;
                println!("已写入。");
            }
            "save" | "edit" => {
                store::write_password(
                    project,
                    identity,
                    Some(title),
                    None,
                    *action == "edit",
                    false,
                    session,
                )?;
            }
            "copy" => crate::clipboard::pass(project, identity, Some(title), false, session)?,
            "show" => crate::clipboard::pass(project, identity, Some(title), true, session)?,
            "exit" => return Ok(()),
            _ => {}
        }
    }
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('$', "$$").replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_actions_do_not_confuse_lookup_and_creation() {
        assert_eq!(local_secret("看看我的github密码"), Some("copy"));
        assert_eq!(local_secret("重新设置GitHub密码"), Some("edit"));
    }
    #[test]
    fn drafts_cannot_write_another_identity() {
        let d = tempfile::tempdir().unwrap();
        let p = Project {
            root: d.path().canonicalize().unwrap(),
        };
        let mut s = Store::open(&p).unwrap();
        assert!(apply(&mut s,"alice",&json!({"items":[{"identity":"bob","category":"work","key":"职位","value":"工程师"}]})).is_err());
        assert!(s.items("bob").unwrap().is_empty());
        apply(&mut s,"alice",&json!({"items":[{"category":"work","key":"职位","value":"工程师"},{"category":"x","key":"empty","value":""}]})).unwrap();
        assert_eq!(s.items("alice").unwrap().len(), 1);
    }
}
