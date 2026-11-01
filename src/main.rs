use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use id_manager::{
    Project, ai, browser, clipboard, config, grants, identity, import, maintenance,
    store::{self, Store},
    ui,
    vault::Session,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "id", version, about = "本地身份管理：账号、密码与个人信息")]
struct Cli {
    #[arg(long, env = "ID_ROOT", default_value = ".", global = true)]
    root: PathBuf,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Args)]
struct Identity {
    #[arg(long = "id", alias = "identity", env = "ID_ENTRY_ID")]
    id: Option<String>,
}
#[derive(Args)]
struct Entry {
    #[command(flatten)]
    identity: Identity,
    #[arg(long, env = "ID_ENTRY_TITLE")]
    title: Option<String>,
    #[arg(long = "user", alias = "username", env = "ID_ENTRY_USER")]
    user: Option<String>,
}
#[derive(Subcommand)]
enum Commands {
    /// 基础信息统一为 ID_info/身份名，并整理本项目旧根条目
    Organize {
        #[command(flatten)]
        identity: Identity,
    },
    /// 新建或补齐身份目录
    New {
        #[command(flatten)]
        identity: Identity,
    },
    /// 管理账号、密码和个人信息
    #[command(alias = "list")]
    Manage {
        #[command(flatten)]
        identity: Identity,
        #[arg(long="type",env="ID_LIST_TYPE",default_value="all",value_parser=["all","account","password","info"])]
        view: String,
        #[arg(long = "cat", env = "ID_LIST_CATEGORY", default_value = "")]
        category: String,
    },
    /// 查看密码条目
    PassList {
        #[command(flatten)]
        identity: Identity,
    },
    /// 复制密码，SSH 下适配终端剪贴板
    Pass {
        #[command(flatten)]
        entry: Entry,
    },
    /// 明文显示密码，需要全局允许和重新验证
    PassShow {
        #[command(flatten)]
        entry: Entry,
    },
    /// 手动录入密码
    PassAddManual {
        #[command(flatten)]
        entry: Entry,
    },
    /// 生成强密码并保存
    PassAdd {
        #[command(flatten)]
        entry: Entry,
    },
    /// 修改已有密码
    PassEdit {
        #[command(flatten)]
        entry: Entry,
    },
    /// 修改密码库主密码
    Passwd {
        #[command(flatten)]
        identity: Identity,
    },
    /// 交互修改设置和 API key
    Settings,
    /// 自测终端剪贴板
    ClipboardCheck,
    /// AI 整理非机密资料，密码操作留在本地
    Ai {
        #[command(flatten)]
        identity: Identity,
        #[arg(long, env = "ID_INPUT_FILE")]
        file: Option<PathBuf>,
    },
    /// 写入或修改非机密资料
    Item {
        #[command(flatten)]
        identity: Identity,
        #[arg(long, env = "ID_ITEM_CATEGORY")]
        cat: Option<String>,
        #[arg(long, env = "ID_ITEM_KEY")]
        key: Option<String>,
        #[arg(long, env = "ID_ITEM_VALUE")]
        value: Option<String>,
        #[arg(long, env = "ID_ITEM_NOTE")]
        note: Option<String>,
    },
    /// 记录事件
    Event {
        #[command(flatten)]
        identity: Identity,
        #[arg(long, env = "ID_EVENT_TITLE")]
        title: Option<String>,
        #[arg(long, env = "ID_EVENT_DETAIL")]
        detail: Option<String>,
    },
    /// 查看当前身份的资料与事件
    Show {
        #[command(flatten)]
        identity: Identity,
    },
    /// 备份所有身份的加密密码库
    Backup,
    /// 各身份密码库状态
    Status,
    /// 安全检查
    Check,
    /// 导入账号表，保存到各身份 KDBX
    Import {
        file: PathBuf,
        #[arg(long = "owner")]
        owners: Vec<String>,
    },
    /// 签发临时凭证
    Grant {
        #[command(flatten)]
        entry: Entry,
        #[arg(long, env = "ID_GRANT_TTL")]
        ttl: Option<u64>,
    },
    /// 使用临时凭证代执行，秘密通过 ID_SECRET 注入子进程
    Use {
        #[arg(long, env = "ID_GRANT_TOKEN")]
        token: Option<String>,
        #[arg(long, env = "ID_GRANT_COMMAND")]
        cmd: Option<String>,
    },
    /// 查看活跃临时凭证
    Grants,
    /// 撤销临时凭证
    Revoke {
        #[arg(long, env = "ID_GRANT_TOKEN")]
        token: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// 修改密码条目 Tags（需要再次验证）
    Tag {
        #[command(flatten)]
        entry: Entry,
        #[arg(long, env = "ID_ENTRY_TAGS")]
        tags: Option<String>,
    },
}
fn ask(value: Option<String>, prompt: &str) -> Result<String> {
    match value.filter(|s| !s.is_empty()) {
        Some(v) => Ok(v),
        None => ui::input(prompt),
    }
}
fn optional(value: Option<String>, prompt: &str) -> Result<String> {
    match value {
        Some(v) => Ok(v),
        None => ui::input(prompt),
    }
}
fn role(project: &Project, args: Identity) -> Result<String> {
    identity::select(project, args.id.as_deref())
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let project = Project::open(&cli.root)?;
    let session = Session::default();
    match cli.command {
        Commands::New { identity } => {
            let name = ask(identity.id, "新身份名: ")?;
            maintenance::new_identity(&project, &name)?;
        }
        Commands::Manage {
            identity,
            view,
            category,
        } => {
            let id = role(&project, identity)?;
            browser::manage(&project, &id, view, category, &session)?;
        }
        Commands::PassList { identity } => {
            let id = role(&project, identity)?;
            browser::manage(&project, &id, "password".into(), String::new(), &session)?;
        }
        Commands::Pass { entry } => {
            let id = role(&project, entry.identity)?;
            clipboard::pass(&project, &id, entry.title.as_deref(), false, &session)?;
        }
        Commands::PassShow { entry } => {
            let id = role(&project, entry.identity)?;
            clipboard::pass(&project, &id, entry.title.as_deref(), true, &session)?;
        }
        Commands::PassAddManual { entry } => {
            let id = role(&project, entry.identity)?;
            import::fill(
                &project,
                &id,
                entry.title.as_deref(),
                entry.user.as_deref(),
                &session,
            )?;
        }
        Commands::PassAdd { entry } => {
            let id = role(&project, entry.identity)?;
            store::write_password(
                &project,
                &id,
                entry.title.as_deref(),
                entry.user.as_deref(),
                false,
                true,
                &session,
            )?;
        }
        Commands::PassEdit { entry } => {
            let id = role(&project, entry.identity)?;
            store::write_password(
                &project,
                &id,
                entry.title.as_deref(),
                entry.user.as_deref(),
                true,
                false,
                &session,
            )?;
        }
        Commands::Passwd { identity } => {
            let id = role(&project, identity)?;
            let vault = identity::vault(&project, &id)?;
            session.unlock(&vault, true)?;
            let first = ui::secret("新主密码: ")?;
            let second = ui::secret("再次输入新主密码: ")?;
            anyhow::ensure!(
                first.bytes() == second.bytes() && !first.is_empty(),
                "两次输入不一致，或主密码为空"
            );
            let _lock = store::write_lock(&project, &id)?;
            store::backup(&project, &id)?;
            session.run(
                &vault,
                "db-edit",
                &["--set-password".into()],
                &[&first, &second],
            )?;
            session.lock();
            println!("主密码已修改。");
        }
        Commands::Settings => config::settings(&project)?,
        Commands::ClipboardCheck => {
            clipboard::check(&project, true)?;
        }
        Commands::Ai { identity, file } => {
            let id = role(&project, identity)?;
            ai::chat(&project, &id, file.as_deref(), &session)?;
        }
        Commands::Item {
            identity,
            cat,
            key,
            value,
            note,
        } => {
            let id = role(&project, identity)?;
            let cat = ask(cat, "分类: ")?;
            let key = ask(key, "名称: ")?;
            let value = ask(value, "内容: ")?;
            let note = optional(note, "备注（可空）: ")?;
            Store::open(&project, &id, &session)?.put(&id, &cat, &key, &value, &note)?;
            println!("已保存。");
        }
        Commands::Event {
            identity,
            title,
            detail,
        } => {
            let id = role(&project, identity)?;
            Store::open(&project, &id, &session)?.event(
                &id,
                &ask(title, "事件标题: ")?,
                &optional(detail, "详情（可空）: ")?,
            )?;
            println!("已记录。");
        }
        Commands::Show { identity } => {
            let id = role(&project, identity)?;
            let s = Store::open(&project, &id, &session)?;
            for item in s.items(&id)? {
                println!(
                    "{} / {} = {}\n  {}",
                    ui::clean(&item.category),
                    ui::clean(&item.key),
                    ui::clean(&item.value),
                    ui::clean(&item.remark())
                );
                for field in &item.fields {
                    println!(
                        "  {} = {}",
                        ui::clean(&field.name),
                        ui::clean(field.value.as_deref().unwrap_or("••••（受保护）"))
                    );
                }
            }
        }
        Commands::Organize { identity } => {
            let id = role(&project, identity)?;
            Store::open(&project, &id, &session)?.organize()?;
        }
        Commands::Backup => {
            for id in identity::names(&project)? {
                let path = store::backup(&project, &id)?;
                println!("{}  {}", id, path.strip_prefix(&project.root)?.display());
            }
        }
        Commands::Status => {
            for id in identity::names(&project)? {
                println!(
                    "{}  KDBX {}",
                    id,
                    if identity::vault(&project, &id).is_ok() {
                        "就绪"
                    } else {
                        "未创建"
                    }
                );
            }
        }
        Commands::Check => id_manager::security::check(&project)?,
        Commands::Import { file, owners } => import::accounts(&project, &file, &owners, &session)?,
        Commands::Grant { entry, ttl } => {
            let id = role(&project, entry.identity)?;
            let title = ask(entry.title, "条目标题: ")?;
            grants::grant(&project, &id, &title, ttl, &session)?;
        }
        Commands::Use { token, cmd } => {
            let token = ask(token, "凭证 token: ")?;
            let cmd = ask(cmd, "执行命令: ")?;
            let code = grants::use_grant(&project, &token, &cmd)?;
            if code != 0 {
                anyhow::bail!("命令退出码 {code}");
            }
        }
        Commands::Grants => grants::list()?,
        Commands::Revoke { token, all } => {
            let token = if all {
                None
            } else {
                Some(ask(token, "凭证 token: ")?)
            };
            grants::revoke(&project, token.as_deref(), all)?;
        }
        Commands::Tag { entry, tags } => {
            let id = role(&project, entry.identity)?;
            let title = ask(entry.title, "条目标题: ")?;
            let tags = match tags {
                Some(tags) => tags,
                None => ui::input("标签（; 分隔，Enter 清空）: ")?,
            };
            maintenance::tag(&project, &id, &title, &tags, &session)?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        let message = error.to_string();
        eprintln!("{}", ui::color(&ui::clean(&message), "31"));
        std::process::exit(if message == "已取消" { 130 } else { 1 });
    }
}
