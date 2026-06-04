use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use clap::{Args, Parser, Subcommand, ValueEnum};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

const DEFAULT_LIMIT: i64 = 50;

#[derive(Parser, Debug)]
#[command(name = "handoff")]
#[command(about = "Local-first agent messaging, context handoff, and background task runtime")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init,
    Join(JoinArgs),
    Whoami(ProjectArgs),
    Actas(AgentArg),
    Active(ProjectArgs),
    Drop(AgentArg),
    Agents(JsonArgs),
    #[command(alias = "team")]
    Team(JsonArgs),
    Send(SendArgs),
    To(SendArgs),
    Post(SendArgs),
    Reply(ReplyArgs),
    Inbox(InboxArgs),
    History(HistoryArgs),
    Show(ShowArgs),
    Mode(ModeArgs),
    Leave(LeaveArgs),
    Reset(ProjectArgs),
    RenameTeam(RenameTeamArgs),
    Context(ContextArgs),
    Run(RunArgs),
    Status(StatusArgs),
    Logs(LogsArgs),
    Result(ResultArgs),
    Cancel(JobIdArg),
    Retry(RetryArgs),
    InstallAlias(InstallAliasArgs),
    #[command(hide = true)]
    Worker(JobIdArg),
}

#[derive(Args, Debug)]
struct JsonArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ProjectArgs {
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct JoinArgs {
    team: String,
    agent: String,
    #[arg(long, value_enum)]
    runtime: Option<Runtime>,
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct AgentArg {
    agent: String,
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug, Clone)]
struct SendArgs {
    agent: String,
    message: Vec<String>,
    #[arg(long = "as")]
    as_agent: Option<String>,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    stdin: bool,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    thread: Option<String>,
    #[arg(long)]
    context: Option<String>,
    #[arg(long = "message")]
    message_text: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ReplyArgs {
    thread_id: String,
    message: Vec<String>,
    #[arg(long = "as")]
    as_agent: Option<String>,
    #[arg(long)]
    stdin: bool,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct InboxArgs {
    #[arg(long = "as")]
    as_agent: Option<String>,
    #[arg(long)]
    unread: bool,
    #[arg(long)]
    all: bool,
    #[arg(long, default_value_t = DEFAULT_LIMIT)]
    limit: i64,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct HistoryArgs {
    #[arg(long = "with")]
    with_agent: Option<String>,
    #[arg(long)]
    team: Option<String>,
    #[arg(long, default_value_t = DEFAULT_LIMIT)]
    limit: i64,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ShowArgs {
    message_id: String,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ModeArgs {
    mode: Option<DeliveryMode>,
    #[arg(long, value_enum)]
    runtime: Option<Runtime>,
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct LeaveArgs {
    #[arg(long)]
    team: Option<String>,
    #[arg(long = "as")]
    as_agent: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct RenameTeamArgs {
    old: String,
    new: String,
    #[arg(long)]
    merge: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ContextArgs {
    #[command(subcommand)]
    command: ContextCommand,
}

#[derive(Subcommand, Debug)]
enum ContextCommand {
    Create(ContextCreateArgs),
    Show(ContextShowArgs),
    List(JsonArgs),
}

#[derive(Args, Debug)]
struct ContextCreateArgs {
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    text: Option<String>,
    #[arg(long)]
    stdin: bool,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    files: Vec<PathBuf>,
    #[arg(long)]
    git_diff: bool,
    #[arg(long)]
    cmd: Option<String>,
    #[arg(long = "as")]
    as_agent: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ContextShowArgs {
    context_id: String,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct RunArgs {
    agent: String,
    #[arg(long)]
    task: String,
    #[arg(long)]
    context: Option<String>,
    #[arg(long)]
    git_diff: bool,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    timeout: Option<u64>,
    #[arg(long = "as")]
    as_agent: Option<String>,
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct StatusArgs {
    job_id: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct LogsArgs {
    job_id: String,
    #[arg(long)]
    tail: Option<i64>,
    #[arg(long)]
    follow: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ResultArgs {
    job_id: String,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct JobIdArg {
    job_id: String,
}

#[derive(Args, Debug)]
struct RetryArgs {
    job_id: String,
    #[arg(long)]
    same_context: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct InstallAliasArgs {
    alias: String,
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Debug, ValueEnum)]
enum Runtime {
    ClaudeCode,
    Codex,
    Gemini,
    Copilot,
    Antigravity,
    Opencode,
    Shell,
    Unknown,
}

impl Runtime {
    fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Copilot => "copilot",
            Self::Antigravity => "antigravity",
            Self::Opencode => "opencode",
            Self::Shell => "shell",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
enum DeliveryMode {
    Monitor,
    Turn,
    Both,
    Off,
}

impl DeliveryMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Monitor => "monitor",
            Self::Turn => "turn",
            Self::Both => "both",
            Self::Off => "off",
        }
    }
}

#[derive(Debug, Clone)]
struct Identity {
    team_id: String,
    team: String,
    agent_id: String,
    agent: String,
    runtime: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let conn = open_db()?;
    ensure_schema(&conn)?;

    match cli.command.unwrap_or(Commands::Inbox(InboxArgs {
        as_agent: None,
        unread: true,
        all: false,
        limit: DEFAULT_LIMIT,
        json: false,
    })) {
        Commands::Init => cmd_init(&conn),
        Commands::Join(args) => cmd_join(&conn, args),
        Commands::Whoami(args) => cmd_whoami(&conn, args),
        Commands::Actas(args) => cmd_actas(&conn, args),
        Commands::Active(args) => cmd_active(&conn, args),
        Commands::Drop(args) => cmd_drop(&conn, args),
        Commands::Agents(args) | Commands::Team(args) => cmd_agents(&conn, args),
        Commands::Send(args) | Commands::To(args) | Commands::Post(args) => {
            cmd_send(&conn, args, "message")
        }
        Commands::Reply(args) => cmd_reply(&conn, args),
        Commands::Inbox(args) => cmd_inbox(&conn, args),
        Commands::History(args) => cmd_history(&conn, args),
        Commands::Show(args) => cmd_show(&conn, args),
        Commands::Mode(args) => cmd_mode(&conn, args),
        Commands::Leave(args) => cmd_leave(&conn, args),
        Commands::Reset(args) => cmd_reset(&conn, args),
        Commands::RenameTeam(args) => cmd_rename_team(&conn, args),
        Commands::Context(args) => cmd_context(&conn, args),
        Commands::Run(args) => cmd_run(&conn, args),
        Commands::Status(args) => cmd_status(&conn, args),
        Commands::Logs(args) => cmd_logs(&conn, args),
        Commands::Result(args) => cmd_result(&conn, args),
        Commands::Cancel(args) => cmd_cancel(&conn, &args.job_id),
        Commands::Retry(args) => cmd_retry(&conn, args),
        Commands::InstallAlias(args) => cmd_install_alias(args),
        Commands::Worker(args) => worker_run(&conn, &args.job_id),
    }
}

fn app_home() -> Result<PathBuf> {
    if let Ok(path) = env::var("HANDOFF_HOME") {
        return Ok(PathBuf::from(path));
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
    Ok(home.join(".handoff"))
}

fn db_path() -> Result<PathBuf> {
    Ok(app_home()?.join("handoff.db"))
}

fn open_db() -> Result<Connection> {
    let home = app_home()?;
    fs::create_dir_all(home.join("logs"))?;
    fs::create_dir_all(home.join("run"))?;
    Connection::open(db_path()?).context("open handoff database")
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        pragma journal_mode = wal;
        create table if not exists events (
          id text primary key,
          type text not null,
          team_id text,
          actor_agent_id text,
          subject_id text,
          payload_json text not null,
          created_at text not null
        );
        create table if not exists teams (
          id text primary key,
          name text not null unique,
          created_at text not null,
          updated_at text not null
        );
        create table if not exists agents (
          id text primary key,
          team_id text not null,
          name text not null,
          runtime text not null,
          created_at text not null,
          updated_at text not null,
          unique(team_id, name)
        );
        create table if not exists project_registrations (
          id text primary key,
          team_id text not null,
          agent_id text not null,
          project_path text not null,
          runtime text not null,
          created_at text not null,
          updated_at text not null,
          unique(team_id, agent_id, project_path, runtime)
        );
        create table if not exists threads (
          id text primary key,
          team_id text not null,
          subject text,
          created_by_agent_id text not null,
          created_at text not null,
          updated_at text not null
        );
        create table if not exists messages (
          id text primary key,
          team_id text not null,
          thread_id text not null,
          from_agent_id text not null,
          to_agent_id text not null,
          kind text not null default 'message',
          context_id text,
          job_id text,
          subject text,
          body text not null,
          created_at text not null
        );
        create table if not exists message_reads (
          message_id text not null,
          agent_id text not null,
          read_at text not null,
          primary key(message_id, agent_id)
        );
        create table if not exists delivery_settings (
          id text primary key,
          project_path text not null,
          runtime text not null,
          mode text not null,
          updated_at text not null,
          unique(project_path, runtime)
        );
        create table if not exists role_locks (
          id text primary key,
          team_id text not null,
          agent_id text not null,
          project_path text not null,
          runtime text not null,
          session_id text,
          process_id integer,
          claimed_at text not null,
          expires_at text,
          unique(team_id, agent_id, project_path, runtime)
        );
        create table if not exists contexts (
          id text primary key,
          team_id text not null,
          created_by_agent_id text not null,
          title text,
          created_at text not null
        );
        create table if not exists context_items (
          id text primary key,
          context_id text not null,
          kind text not null,
          label text,
          source text,
          content text,
          content_hash text,
          metadata_json text not null,
          created_at text not null
        );
        create table if not exists jobs (
          id text primary key,
          team_id text not null,
          thread_id text not null,
          task_message_id text not null,
          context_id text,
          requested_by_agent_id text not null,
          target_agent_id text not null,
          runtime text not null,
          state text not null,
          retry_of_job_id text,
          timeout_seconds integer,
          process_id integer,
          created_at text not null,
          started_at text,
          finished_at text,
          result_message_id text,
          failure_code text,
          failure_message text
        );
        create table if not exists job_logs (
          id text primary key,
          job_id text not null,
          stream text not null,
          line text not null,
          created_at text not null
        );
        "#,
    )?;
    Ok(())
}

fn cmd_init(_conn: &Connection) -> Result<()> {
    println!("Initialized handoff at {}", app_home()?.display());
    println!("Database: {}", db_path()?.display());
    Ok(())
}

fn cmd_join(conn: &Connection, args: JoinArgs) -> Result<()> {
    let runtime = args.runtime.unwrap_or_else(detect_runtime);
    let project = project_path(args.project)?;
    let now = now();
    let team_id = get_or_create_team(conn, &args.team)?;
    let agent_id = get_or_create_agent(conn, &team_id, &args.agent, runtime.as_str())?;
    conn.execute(
        "insert into project_registrations (id, team_id, agent_id, project_path, runtime, created_at, updated_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         on conflict(team_id, agent_id, project_path, runtime)
         do update set updated_at=excluded.updated_at",
        params![id(), team_id, agent_id, project, runtime.as_str(), now],
    )?;
    append_event(
        conn,
        "project.registered",
        Some(&team_id),
        Some(&agent_id),
        Some(&project),
        json!({"team": args.team, "agent": args.agent, "runtime": runtime.as_str(), "project": project}),
    )?;
    if args.json {
        print_json(
            json!({"ok": true, "team": args.team, "agent": args.agent, "runtime": runtime.as_str(), "project": project}),
        );
    } else {
        println!(
            "Joined team '{}' as '{}' ({})",
            args.team,
            args.agent,
            runtime.as_str()
        );
    }
    Ok(())
}

fn cmd_whoami(conn: &Connection, args: ProjectArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let identities = identities_for_project(conn, &project)?;
    if args.json {
        print_json(
            json!({"ok": true, "project": project, "identities": identities_to_json(&identities)}),
        );
    } else if identities.is_empty() {
        println!("not joined: {project}");
    } else {
        for identity in identities {
            println!(
                "{} / {} ({})",
                identity.team, identity.agent, identity.runtime
            );
        }
    }
    Ok(())
}

fn cmd_actas(conn: &Connection, args: AgentArg) -> Result<()> {
    let project = project_path(args.project)?;
    let identities = identities_for_project(conn, &project)?;
    let identity = identities
        .into_iter()
        .find(|item| item.agent == args.agent)
        .ok_or_else(|| anyhow!("unknown agent identity for this project: {}", args.agent))?;
    let now = now();
    conn.execute(
        "insert into role_locks (id, team_id, agent_id, project_path, runtime, session_id, process_id, claimed_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         on conflict(team_id, agent_id, project_path, runtime)
         do update set session_id=excluded.session_id, process_id=excluded.process_id, claimed_at=excluded.claimed_at",
        params![
            id(),
            identity.team_id,
            identity.agent_id,
            project,
            identity.runtime,
            env::var("HANDOFF_SESSION_ID").ok(),
            std::process::id() as i64,
            now
        ],
    )?;
    append_event(
        conn,
        "agent.role_claimed",
        Some(&identity.team_id),
        Some(&identity.agent_id),
        Some(&project),
        json!({"project": project, "agent": identity.agent}),
    )?;
    if args.json {
        print_json(json!({"ok": true, "active": identity.agent}));
    } else {
        println!("Active role: {}", identity.agent);
    }
    Ok(())
}

fn cmd_active(conn: &Connection, args: ProjectArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let active = active_identity(conn, &project)?;
    if args.json {
        print_json(json!({"ok": true, "active": active.as_ref().map(identity_json)}));
    } else if let Some(identity) = active {
        println!(
            "{} / {} ({})",
            identity.team, identity.agent, identity.runtime
        );
    } else {
        println!("No active role");
    }
    Ok(())
}

fn cmd_drop(conn: &Connection, args: AgentArg) -> Result<()> {
    let project = project_path(args.project)?;
    let identities = identities_for_project(conn, &project)?;
    let identity = identities
        .into_iter()
        .find(|item| item.agent == args.agent)
        .ok_or_else(|| anyhow!("unknown agent identity for this project: {}", args.agent))?;
    conn.execute(
        "delete from project_registrations where team_id=?1 and agent_id=?2 and project_path=?3 and runtime=?4",
        params![identity.team_id, identity.agent_id, project, identity.runtime],
    )?;
    conn.execute(
        "delete from role_locks where team_id=?1 and agent_id=?2 and project_path=?3 and runtime=?4",
        params![identity.team_id, identity.agent_id, project, identity.runtime],
    )?;
    append_event(
        conn,
        "agent.role_released",
        Some(&identity.team_id),
        Some(&identity.agent_id),
        Some(&project),
        json!({"project": project, "agent": identity.agent}),
    )?;
    if args.json {
        print_json(json!({"ok": true, "dropped": identity.agent}));
    } else {
        println!("Dropped role '{}'", identity.agent);
    }
    Ok(())
}

fn cmd_agents(conn: &Connection, args: JsonArgs) -> Result<()> {
    let identity = resolve_identity(conn, None, None, None)?;
    let agents = list_agents(conn, &identity.team_id)?;
    if args.json {
        print_json(json!({"ok": true, "team": identity.team, "agents": agents}));
    } else {
        for agent in agents {
            println!(
                "{} ({})",
                agent["name"].as_str().unwrap_or_default(),
                agent["runtime"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn cmd_send(conn: &Connection, args: SendArgs, default_kind: &str) -> Result<()> {
    let body = read_message_body(
        &args.message,
        args.stdin,
        args.file.as_deref(),
        args.message_text.as_deref(),
    )?;
    let sender = resolve_identity(conn, args.as_agent.as_deref(), args.team.as_deref(), None)?;
    let recipient = agent_by_name(conn, &sender.team_id, &args.agent)?;
    let kind = if args.context.is_some() {
        "context"
    } else {
        default_kind
    };
    let message_id = create_message(
        conn,
        &sender,
        &recipient.agent_id,
        args.thread.as_deref(),
        kind,
        args.context.as_deref(),
        None,
        args.subject.as_deref(),
        &body,
    )?;
    if args.json {
        print_json(json!({"ok": true, "message_id": message_id}));
    } else {
        println!("sent {message_id}");
    }
    Ok(())
}

fn cmd_reply(conn: &Connection, args: ReplyArgs) -> Result<()> {
    let body = read_message_body(&args.message, args.stdin, args.file.as_deref(), None)?;
    let sender = resolve_identity(conn, args.as_agent.as_deref(), None, None)?;
    let recipient_id = reply_recipient(conn, &args.thread_id, &sender.agent_id)?;
    let message_id = create_message(
        conn,
        &sender,
        &recipient_id,
        Some(&args.thread_id),
        "message",
        None,
        None,
        args.subject.as_deref(),
        &body,
    )?;
    if args.json {
        print_json(json!({"ok": true, "message_id": message_id, "thread_id": args.thread_id}));
    } else {
        println!("replied {message_id}");
    }
    Ok(())
}

fn cmd_inbox(conn: &Connection, args: InboxArgs) -> Result<()> {
    let identity = resolve_identity(conn, args.as_agent.as_deref(), None, None)?;
    let unread_only = !args.all;
    let messages = inbox_messages(conn, &identity.agent_id, unread_only, args.limit)?;
    for message in &messages {
        conn.execute(
            "insert or ignore into message_reads (message_id, agent_id, read_at) values (?1, ?2, ?3)",
            params![message["id"].as_str(), identity.agent_id, now()],
        )?;
    }
    if args.json {
        print_json(json!({"ok": true, "messages": messages}));
    } else if messages.is_empty() {
        println!("No messages");
    } else {
        for message in messages {
            println!(
                "[{}] {} -> {}: {}",
                message["id"].as_str().unwrap_or_default(),
                message["from"].as_str().unwrap_or_default(),
                message["to"].as_str().unwrap_or_default(),
                first_line(message["body"].as_str().unwrap_or_default())
            );
        }
    }
    Ok(())
}

fn cmd_history(conn: &Connection, args: HistoryArgs) -> Result<()> {
    let identity = resolve_identity(conn, None, args.team.as_deref(), None)?;
    let messages = history_messages(
        conn,
        &identity.team_id,
        args.with_agent.as_deref(),
        args.limit,
    )?;
    if args.json {
        print_json(json!({"ok": true, "messages": messages}));
    } else {
        for message in messages {
            println!(
                "[{}] {} -> {} ({}) {}",
                message["id"].as_str().unwrap_or_default(),
                message["from"].as_str().unwrap_or_default(),
                message["to"].as_str().unwrap_or_default(),
                message["kind"].as_str().unwrap_or_default(),
                first_line(message["body"].as_str().unwrap_or_default())
            );
        }
    }
    Ok(())
}

fn cmd_show(conn: &Connection, args: ShowArgs) -> Result<()> {
    let message = message_json(conn, &args.message_id)?
        .ok_or_else(|| anyhow!("unknown message: {}", args.message_id))?;
    if args.json {
        print_json(json!({"ok": true, "message": message}));
    } else {
        println!("{}", serde_json::to_string_pretty(&message)?);
    }
    Ok(())
}

fn cmd_mode(conn: &Connection, args: ModeArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let runtime = args.runtime.unwrap_or_else(detect_runtime);
    if let Some(mode) = args.mode {
        validate_mode(runtime.as_str(), mode.as_str())?;
        conn.execute(
            "insert into delivery_settings (id, project_path, runtime, mode, updated_at)
             values (?1, ?2, ?3, ?4, ?5)
             on conflict(project_path, runtime) do update set mode=excluded.mode, updated_at=excluded.updated_at",
            params![id(), project, runtime.as_str(), mode.as_str(), now()],
        )?;
        append_event(
            conn,
            "delivery.mode_set",
            None,
            None,
            Some(&project),
            json!({"runtime": runtime.as_str(), "mode": mode.as_str()}),
        )?;
    }
    let current: Option<String> = conn
        .query_row(
            "select mode from delivery_settings where project_path=?1 and runtime=?2",
            params![project, runtime.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if args.json {
        print_json(
            json!({"ok": true, "project": project, "runtime": runtime.as_str(), "mode": current.unwrap_or_else(|| "off".into())}),
        );
    } else {
        println!(
            "{}: {}",
            runtime.as_str(),
            current.unwrap_or_else(|| "off".into())
        );
    }
    Ok(())
}

fn cmd_leave(conn: &Connection, args: LeaveArgs) -> Result<()> {
    let identity = resolve_identity(conn, args.as_agent.as_deref(), args.team.as_deref(), None)?;
    let project = project_path(None)?;
    conn.execute(
        "delete from project_registrations where team_id=?1 and agent_id=?2 and project_path=?3",
        params![identity.team_id, identity.agent_id, project],
    )?;
    if args.json {
        print_json(json!({"ok": true}));
    } else {
        println!("Left {} as {}", identity.team, identity.agent);
    }
    Ok(())
}

fn cmd_reset(conn: &Connection, args: ProjectArgs) -> Result<()> {
    let project = project_path(args.project)?;
    conn.execute(
        "delete from project_registrations where project_path=?1",
        params![project],
    )?;
    conn.execute(
        "delete from role_locks where project_path=?1",
        params![project],
    )?;
    append_event(
        conn,
        "project.reset",
        None,
        None,
        Some(&project),
        json!({"project": project}),
    )?;
    if args.json {
        print_json(json!({"ok": true, "project": project}));
    } else {
        println!("Reset registrations for {project}");
    }
    Ok(())
}

fn cmd_rename_team(conn: &Connection, args: RenameTeamArgs) -> Result<()> {
    let old_id: String = conn
        .query_row(
            "select id from teams where name=?1",
            params![args.old],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown team: {}", args.old))?;
    let existing: Option<String> = conn
        .query_row(
            "select id from teams where name=?1",
            params![args.new],
            |row| row.get(0),
        )
        .optional()?;
    if existing.is_some() && !args.merge {
        bail!("team already exists: {}", args.new);
    }
    conn.execute(
        "update teams set name=?1, updated_at=?2 where id=?3",
        params![args.new, now(), old_id],
    )?;
    append_event(
        conn,
        "team.renamed",
        Some(&old_id),
        None,
        Some(&old_id),
        json!({"old": args.old, "new": args.new}),
    )?;
    if args.json {
        print_json(json!({"ok": true, "team": args.new}));
    } else {
        println!("Renamed team to '{}'", args.new);
    }
    Ok(())
}

fn cmd_context(conn: &Connection, args: ContextArgs) -> Result<()> {
    match args.command {
        ContextCommand::Create(args) => cmd_context_create(conn, args),
        ContextCommand::Show(args) => cmd_context_show(conn, args),
        ContextCommand::List(args) => cmd_context_list(conn, args),
    }
}

fn cmd_context_create(conn: &Connection, args: ContextCreateArgs) -> Result<()> {
    let identity = resolve_identity(conn, args.as_agent.as_deref(), None, None)?;
    let context_id = create_context(conn, &identity, args.title.as_deref())?;
    add_context_inputs(
        conn,
        &context_id,
        args.text.as_deref(),
        args.stdin,
        args.file.as_deref(),
        &args.files,
        args.git_diff,
        args.cmd.as_deref(),
    )?;
    append_event(
        conn,
        "context.created",
        Some(&identity.team_id),
        Some(&identity.agent_id),
        Some(&context_id),
        json!({"context_id": context_id}),
    )?;
    if args.json {
        print_json(json!({"ok": true, "context_id": context_id}));
    } else {
        println!("context {context_id}");
    }
    Ok(())
}

fn cmd_context_show(conn: &Connection, args: ContextShowArgs) -> Result<()> {
    let context = context_json(conn, &args.context_id)?;
    if args.json {
        print_json(json!({"ok": true, "context": context}));
    } else {
        println!("{}", serde_json::to_string_pretty(&context)?);
    }
    Ok(())
}

fn cmd_context_list(conn: &Connection, args: JsonArgs) -> Result<()> {
    let identity = resolve_identity(conn, None, None, None)?;
    let mut stmt = conn.prepare(
        "select id, title, created_at from contexts where team_id=?1 order by created_at desc limit 50",
    )?;
    let rows = stmt.query_map(params![identity.team_id], |row| {
        Ok(json!({"id": row.get::<_, String>(0)?, "title": row.get::<_, Option<String>>(1)?, "created_at": row.get::<_, String>(2)?}))
    })?;
    let contexts = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    if args.json {
        print_json(json!({"ok": true, "contexts": contexts}));
    } else {
        for item in contexts {
            println!(
                "[{}] {}",
                item["id"].as_str().unwrap_or_default(),
                item["title"].as_str().unwrap_or("")
            );
        }
    }
    Ok(())
}

fn cmd_run(conn: &Connection, args: RunArgs) -> Result<()> {
    let sender = resolve_identity(conn, args.as_agent.as_deref(), None, None)?;
    let target = agent_by_name(conn, &sender.team_id, &args.agent)?;
    let context_id = if let Some(context_id) = args.context {
        Some(context_id)
    } else if args.git_diff || args.file.is_some() {
        let created = create_context(conn, &sender, args.subject.as_deref())?;
        add_context_inputs(
            conn,
            &created,
            None,
            false,
            args.file.as_deref(),
            &[],
            args.git_diff,
            None,
        )?;
        Some(created)
    } else {
        None
    };
    let task_message_id = create_message(
        conn,
        &sender,
        &target.agent_id,
        None,
        "task",
        context_id.as_deref(),
        None,
        args.subject.as_deref(),
        &args.task,
    )?;
    let thread_id = message_thread(conn, &task_message_id)?;
    let job_id = id();
    conn.execute(
        "insert into jobs (id, team_id, thread_id, task_message_id, context_id, requested_by_agent_id, target_agent_id, runtime, state, timeout_seconds, created_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', ?9, ?10)",
        params![
            job_id,
            sender.team_id,
            thread_id,
            task_message_id,
            context_id,
            sender.agent_id,
            target.agent_id,
            target.runtime,
            args.timeout.map(|value| value as i64),
            now()
        ],
    )?;
    conn.execute(
        "update messages set job_id=?1 where id=?2",
        params![job_id, task_message_id],
    )?;
    append_event(
        conn,
        "job.created",
        Some(&sender.team_id),
        Some(&sender.agent_id),
        Some(&job_id),
        json!({"job_id": job_id}),
    )?;
    spawn_worker(&job_id)?;
    if args.json {
        print_json(json!({"ok": true, "job_id": job_id}));
    } else {
        println!("job {job_id}");
    }
    Ok(())
}

fn cmd_status(conn: &Connection, args: StatusArgs) -> Result<()> {
    if let Some(job_id) = args.job_id {
        let status = job_json(conn, &job_id)?.ok_or_else(|| anyhow!("unknown job: {job_id}"))?;
        if args.json {
            print_json(json!({"ok": true, "job": status}));
        } else {
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
    } else {
        let jobs = recent_jobs(conn)?;
        if args.json {
            print_json(json!({"ok": true, "jobs": jobs}));
        } else {
            for job in jobs {
                println!(
                    "[{}] {}",
                    job["id"].as_str().unwrap_or_default(),
                    job["state"].as_str().unwrap_or_default()
                );
            }
        }
    }
    Ok(())
}

fn cmd_logs(conn: &Connection, args: LogsArgs) -> Result<()> {
    let logs = job_logs(conn, &args.job_id, args.tail.unwrap_or(200))?;
    if args.json {
        print_json(json!({"ok": true, "logs": logs}));
    } else {
        for log in logs {
            println!(
                "{} {}",
                log["stream"].as_str().unwrap_or_default(),
                log["line"].as_str().unwrap_or_default()
            );
        }
        if args.follow {
            eprintln!("--follow is accepted but currently prints the current log snapshot.");
        }
    }
    Ok(())
}

fn cmd_result(conn: &Connection, args: ResultArgs) -> Result<()> {
    let job =
        job_json(conn, &args.job_id)?.ok_or_else(|| anyhow!("unknown job: {}", args.job_id))?;
    if let Some(message_id) = job["result_message_id"].as_str() {
        let message =
            message_json(conn, message_id)?.ok_or_else(|| anyhow!("missing result message"))?;
        if args.json {
            print_json(json!({"ok": true, "result": message}));
        } else {
            println!("{}", message["body"].as_str().unwrap_or_default());
        }
        Ok(())
    } else {
        bail!("job_not_finished: job has no result yet");
    }
}

fn cmd_cancel(conn: &Connection, job_id: &str) -> Result<()> {
    let job = job_json(conn, job_id)?.ok_or_else(|| anyhow!("unknown job: {job_id}"))?;
    let state = job["state"].as_str().unwrap_or_default();
    if matches!(
        state,
        "succeeded" | "failed" | "cancelled" | "timeout" | "blocked"
    ) {
        println!("job already finished: {state}");
        return Ok(());
    }
    if let Some(pid) = job["process_id"].as_i64() {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    conn.execute(
        "update jobs set state='cancelled', finished_at=?1 where id=?2",
        params![now(), job_id],
    )?;
    append_event(
        conn,
        "job.cancelled",
        None,
        None,
        Some(job_id),
        json!({"job_id": job_id}),
    )?;
    println!("cancelled {job_id}");
    Ok(())
}

fn cmd_retry(conn: &Connection, args: RetryArgs) -> Result<()> {
    let job =
        job_json(conn, &args.job_id)?.ok_or_else(|| anyhow!("unknown job: {}", args.job_id))?;
    let task_message_id = job["task_message_id"]
        .as_str()
        .ok_or_else(|| anyhow!("job has no task message"))?;
    let task =
        message_json(conn, task_message_id)?.ok_or_else(|| anyhow!("missing task message"))?;
    let target = task["to"]
        .as_str()
        .ok_or_else(|| anyhow!("missing target"))?
        .to_string();
    let run_args = RunArgs {
        agent: target,
        task: task["body"].as_str().unwrap_or_default().to_string(),
        context: job["context_id"].as_str().map(ToOwned::to_owned),
        git_diff: false,
        file: None,
        timeout: job["timeout_seconds"].as_i64().map(|value| value as u64),
        as_agent: task["from"].as_str().map(ToOwned::to_owned),
        subject: task["subject"].as_str().map(ToOwned::to_owned),
        json: args.json,
    };
    cmd_run(conn, run_args)
}

fn cmd_install_alias(args: InstallAliasArgs) -> Result<()> {
    let bin_dir = app_home()?.join("bin");
    fs::create_dir_all(&bin_dir)?;
    let alias_path = bin_dir.join(&args.alias);
    let exe = env::current_exe()?;
    #[cfg(unix)]
    {
        if alias_path.exists() {
            fs::remove_file(&alias_path)?;
        }
        std::os::unix::fs::symlink(&exe, &alias_path)?;
    }
    #[cfg(not(unix))]
    {
        fs::copy(&exe, &alias_path)?;
    }
    if args.json {
        print_json(json!({"ok": true, "alias": args.alias, "path": alias_path}));
    } else {
        println!("Installed alias at {}", alias_path.display());
    }
    Ok(())
}

fn worker_run(conn: &Connection, job_id: &str) -> Result<()> {
    let job = job_row(conn, job_id)?.ok_or_else(|| anyhow!("unknown job: {job_id}"))?;
    set_job_state(conn, job_id, "starting", None, None)?;
    let task =
        message_json(conn, &job.task_message_id)?.ok_or_else(|| anyhow!("missing task message"))?;
    let task_body = task["body"].as_str().unwrap_or_default().to_string();
    let context_text = job
        .context_id
        .as_deref()
        .map(|context_id| context_plaintext(conn, context_id))
        .transpose()?
        .unwrap_or_default();
    let command = adapter_command(&job.runtime, &job.target_agent_name, &task_body);
    let Some(command) = command else {
        set_job_state(
            conn,
            job_id,
            "blocked",
            Some("adapter_unavailable"),
            Some(&format!(
                "No adapter command configured for runtime '{}' agent '{}'",
                job.runtime, job.target_agent_name
            )),
        )?;
        return Ok(());
    };
    set_job_state(conn, job_id, "running", None, None)?;
    append_log(conn, job_id, "adapter", &format!("running: {command}"))?;
    let child = shell_command(&command)
        .env("HANDOFF_JOB_ID", job_id)
        .env("HANDOFF_TASK", &task_body)
        .env("HANDOFF_CONTEXT", &context_text)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn adapter command")?;
    conn.execute(
        "update jobs set process_id=?1 where id=?2",
        params![child.id() as i64, job_id],
    )?;
    let output = child
        .wait_with_output()
        .context("wait for adapter command")?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    for line in stdout.lines() {
        append_log(conn, job_id, "stdout", line)?;
    }
    for line in stderr.lines() {
        append_log(conn, job_id, "stderr", line)?;
    }
    if output.status.success() {
        let result_body = if stdout.trim().is_empty() {
            "Task completed with no output.".to_string()
        } else {
            stdout
        };
        let target_identity = Identity {
            team_id: job.team_id.clone(),
            team: job.team_name.clone(),
            agent_id: job.target_agent_id.clone(),
            agent: job.target_agent_name.clone(),
            runtime: job.runtime.clone(),
        };
        let result_message_id = create_message(
            conn,
            &target_identity,
            &job.requested_by_agent_id,
            Some(&job.thread_id),
            "result",
            job.context_id.as_deref(),
            Some(job_id),
            Some("Task result"),
            &result_body,
        )?;
        conn.execute(
            "update jobs set state='succeeded', finished_at=?1, result_message_id=?2, process_id=null where id=?3",
            params![now(), result_message_id, job_id],
        )?;
        append_event(
            conn,
            "job.completed",
            Some(&job.team_id),
            Some(&job.target_agent_id),
            Some(job_id),
            json!({"job_id": job_id}),
        )?;
    } else {
        conn.execute(
            "update jobs set state='failed', finished_at=?1, failure_code='adapter_failed', failure_message=?2, process_id=null where id=?3",
            params![now(), stderr, job_id],
        )?;
        append_event(
            conn,
            "job.failed",
            Some(&job.team_id),
            Some(&job.target_agent_id),
            Some(job_id),
            json!({"job_id": job_id}),
        )?;
    }
    Ok(())
}

#[derive(Debug)]
struct AgentRef {
    agent_id: String,
    runtime: String,
}

#[derive(Debug)]
struct JobRow {
    team_id: String,
    team_name: String,
    thread_id: String,
    task_message_id: String,
    context_id: Option<String>,
    requested_by_agent_id: String,
    target_agent_id: String,
    target_agent_name: String,
    runtime: String,
}

fn get_or_create_team(conn: &Connection, name: &str) -> Result<String> {
    if let Some(existing) = conn
        .query_row("select id from teams where name=?1", params![name], |row| {
            row.get(0)
        })
        .optional()?
    {
        return Ok(existing);
    }
    let team_id = id();
    let now = now();
    conn.execute(
        "insert into teams (id, name, created_at, updated_at) values (?1, ?2, ?3, ?3)",
        params![team_id, name, now],
    )?;
    append_event(
        conn,
        "team.created",
        Some(&team_id),
        None,
        Some(&team_id),
        json!({"name": name}),
    )?;
    Ok(team_id)
}

fn get_or_create_agent(
    conn: &Connection,
    team_id: &str,
    name: &str,
    runtime: &str,
) -> Result<String> {
    if let Some(existing) = conn
        .query_row(
            "select id from agents where team_id=?1 and name=?2",
            params![team_id, name],
            |row| row.get(0),
        )
        .optional()?
    {
        conn.execute(
            "update agents set runtime=?1, updated_at=?2 where id=?3",
            params![runtime, now(), existing],
        )?;
        return Ok(existing);
    }
    let agent_id = id();
    let now = now();
    conn.execute(
        "insert into agents (id, team_id, name, runtime, created_at, updated_at) values (?1, ?2, ?3, ?4, ?5, ?5)",
        params![agent_id, team_id, name, runtime, now],
    )?;
    append_event(
        conn,
        "agent.joined",
        Some(team_id),
        Some(&agent_id),
        Some(&agent_id),
        json!({"name": name, "runtime": runtime}),
    )?;
    Ok(agent_id)
}

fn identities_for_project(conn: &Connection, project: &str) -> Result<Vec<Identity>> {
    let mut stmt = conn.prepare(
        "select t.id, t.name, a.id, a.name, pr.runtime
         from project_registrations pr
         join teams t on t.id=pr.team_id
         join agents a on a.id=pr.agent_id
         where pr.project_path=?1
         order by t.name, a.name",
    )?;
    let rows = stmt.query_map(params![project], |row| {
        Ok(Identity {
            team_id: row.get(0)?,
            team: row.get(1)?,
            agent_id: row.get(2)?,
            agent: row.get(3)?,
            runtime: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn active_identity(conn: &Connection, project: &str) -> Result<Option<Identity>> {
    conn.query_row(
        "select t.id, t.name, a.id, a.name, rl.runtime
         from role_locks rl
         join teams t on t.id=rl.team_id
         join agents a on a.id=rl.agent_id
         where rl.project_path=?1
         order by rl.claimed_at desc
         limit 1",
        params![project],
        |row| {
            Ok(Identity {
                team_id: row.get(0)?,
                team: row.get(1)?,
                agent_id: row.get(2)?,
                agent: row.get(3)?,
                runtime: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn resolve_identity(
    conn: &Connection,
    as_agent: Option<&str>,
    team: Option<&str>,
    project_override: Option<&str>,
) -> Result<Identity> {
    let project = project_override
        .map(ToOwned::to_owned)
        .unwrap_or(project_path(None)?);
    if as_agent.is_none() {
        if let Some(active) = active_identity(conn, &project)? {
            if team.is_none_or(|team| team == active.team) {
                return Ok(active);
            }
        }
    }
    let mut identities = identities_for_project(conn, &project)?;
    if let Some(team) = team {
        identities.retain(|item| item.team == team);
    }
    if let Some(as_agent) = as_agent {
        identities.retain(|item| item.agent == as_agent);
    }
    match identities.len() {
        0 => bail!("not_joined: no matching identity for project {project}"),
        1 => Ok(identities.remove(0)),
        _ => bail!("multiple_identities: use --as <agent>"),
    }
}

fn agent_by_name(conn: &Connection, team_id: &str, name: &str) -> Result<AgentRef> {
    conn.query_row(
        "select id, name, runtime from agents where team_id=?1 and name=?2",
        params![team_id, name],
        |row| {
            Ok(AgentRef {
                agent_id: row.get(0)?,
                runtime: row.get(2)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow!("unknown_agent: {name}"))
}

fn list_agents(conn: &Connection, team_id: &str) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn
        .prepare("select name, runtime, created_at from agents where team_id=?1 order by name")?;
    let rows = stmt.query_map(params![team_id], |row| {
        Ok(json!({"name": row.get::<_, String>(0)?, "runtime": row.get::<_, String>(1)?, "created_at": row.get::<_, String>(2)?}))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn create_message(
    conn: &Connection,
    sender: &Identity,
    recipient_agent_id: &str,
    thread_id: Option<&str>,
    kind: &str,
    context_id: Option<&str>,
    job_id: Option<&str>,
    subject: Option<&str>,
    body: &str,
) -> Result<String> {
    let thread_id = if let Some(thread_id) = thread_id {
        thread_id.to_string()
    } else {
        let thread_id = id();
        let now = now();
        conn.execute(
            "insert into threads (id, team_id, subject, created_by_agent_id, created_at, updated_at) values (?1, ?2, ?3, ?4, ?5, ?5)",
            params![thread_id, sender.team_id, subject, sender.agent_id, now],
        )?;
        append_event(
            conn,
            "thread.created",
            Some(&sender.team_id),
            Some(&sender.agent_id),
            Some(&thread_id),
            json!({"subject": subject}),
        )?;
        thread_id
    };
    let message_id = id();
    let now = now();
    conn.execute(
        "insert into messages (id, team_id, thread_id, from_agent_id, to_agent_id, kind, context_id, job_id, subject, body, created_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![message_id, sender.team_id, thread_id, sender.agent_id, recipient_agent_id, kind, context_id, job_id, subject, body, now],
    )?;
    conn.execute(
        "update threads set updated_at=?1 where id=?2",
        params![now, thread_id],
    )?;
    append_event(
        conn,
        "message.sent",
        Some(&sender.team_id),
        Some(&sender.agent_id),
        Some(&message_id),
        json!({"kind": kind, "thread_id": thread_id}),
    )?;
    Ok(message_id)
}

fn reply_recipient(conn: &Connection, thread_id: &str, sender_id: &str) -> Result<String> {
    let row: Option<(String, String)> = conn
        .query_row(
            "select from_agent_id, to_agent_id from messages where thread_id=?1 order by created_at desc limit 1",
            params![thread_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((from, to)) = row else {
        bail!("unknown_thread: {thread_id}");
    };
    if from == sender_id { Ok(to) } else { Ok(from) }
}

fn inbox_messages(
    conn: &Connection,
    agent_id: &str,
    unread_only: bool,
    limit: i64,
) -> Result<Vec<serde_json::Value>> {
    let sql = if unread_only {
        "select m.id from messages m left join message_reads r on r.message_id=m.id and r.agent_id=?1 where m.to_agent_id=?1 and r.message_id is null order by m.created_at asc limit ?2"
    } else {
        "select m.id from messages m where m.to_agent_id=?1 order by m.created_at desc limit ?2"
    };
    let mut stmt = conn.prepare(sql)?;
    let ids = stmt
        .query_map(params![agent_id, limit], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|message_id| {
            message_json(conn, &message_id)?.ok_or_else(|| anyhow!("missing message"))
        })
        .collect()
}

fn history_messages(
    conn: &Connection,
    team_id: &str,
    with_agent: Option<&str>,
    limit: i64,
) -> Result<Vec<serde_json::Value>> {
    let ids = if let Some(agent_name) = with_agent {
        let agent = agent_by_name(conn, team_id, agent_name)?;
        let mut stmt = conn.prepare(
            "select id from messages where team_id=?1 and (from_agent_id=?2 or to_agent_id=?2) order by created_at desc limit ?3",
        )?;
        stmt.query_map(params![team_id, agent.agent_id, limit], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        let mut stmt = conn.prepare(
            "select id from messages where team_id=?1 order by created_at desc limit ?2",
        )?;
        stmt.query_map(params![team_id, limit], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    ids.into_iter()
        .map(|message_id| {
            message_json(conn, &message_id)?.ok_or_else(|| anyhow!("missing message"))
        })
        .collect()
}

fn message_json(conn: &Connection, message_id: &str) -> Result<Option<serde_json::Value>> {
    conn.query_row(
        "select m.id, m.thread_id, t.name, fa.name, ta.name, m.kind, m.context_id, m.job_id, m.subject, m.body, m.created_at
         from messages m
         join teams t on t.id=m.team_id
         join agents fa on fa.id=m.from_agent_id
         join agents ta on ta.id=m.to_agent_id
         where m.id=?1",
        params![message_id],
        |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "thread_id": row.get::<_, String>(1)?,
                "team": row.get::<_, String>(2)?,
                "from": row.get::<_, String>(3)?,
                "to": row.get::<_, String>(4)?,
                "kind": row.get::<_, String>(5)?,
                "context_id": row.get::<_, Option<String>>(6)?,
                "job_id": row.get::<_, Option<String>>(7)?,
                "subject": row.get::<_, Option<String>>(8)?,
                "body": row.get::<_, String>(9)?,
                "created_at": row.get::<_, String>(10)?,
            }))
        },
    )
    .optional()
    .map_err(Into::into)
}

fn message_thread(conn: &Connection, message_id: &str) -> Result<String> {
    conn.query_row(
        "select thread_id from messages where id=?1",
        params![message_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn create_context(conn: &Connection, identity: &Identity, title: Option<&str>) -> Result<String> {
    let context_id = id();
    conn.execute(
        "insert into contexts (id, team_id, created_by_agent_id, title, created_at) values (?1, ?2, ?3, ?4, ?5)",
        params![context_id, identity.team_id, identity.agent_id, title, now()],
    )?;
    Ok(context_id)
}

fn add_context_inputs(
    conn: &Connection,
    context_id: &str,
    text: Option<&str>,
    read_stdin: bool,
    file: Option<&Path>,
    files: &[PathBuf],
    git_diff: bool,
    cmd: Option<&str>,
) -> Result<()> {
    let mut added = 0;
    if let Some(text) = text {
        add_context_item(
            conn,
            context_id,
            "text",
            Some("text"),
            None,
            text,
            json!({}),
        )?;
        added += 1;
    }
    if read_stdin {
        let content = read_all_stdin()?;
        add_context_item(
            conn,
            context_id,
            "stdin",
            Some("stdin"),
            None,
            &content,
            json!({}),
        )?;
        added += 1;
    }
    if let Some(file) = file {
        add_file_context(conn, context_id, file)?;
        added += 1;
    }
    for file in files {
        add_file_context(conn, context_id, file)?;
        added += 1;
    }
    if git_diff {
        let output = Command::new("git")
            .args(["diff", "--no-ext-diff", "HEAD"])
            .output()
            .or_else(|_| Command::new("git").args(["diff", "--no-ext-diff"]).output())
            .context("capture git diff")?;
        let content = String::from_utf8_lossy(&output.stdout).to_string();
        add_context_item(
            conn,
            context_id,
            "git_diff",
            Some("git diff"),
            Some("git diff --no-ext-diff HEAD"),
            &content,
            json!({"status": output.status.code()}),
        )?;
        added += 1;
    }
    if let Some(cmd) = cmd {
        let output = shell_command(cmd)
            .output()
            .context("capture command output")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let content = format!("$ {cmd}\n\n[stdout]\n{stdout}\n[stderr]\n{stderr}");
        add_context_item(
            conn,
            context_id,
            "command_output",
            Some(cmd),
            Some(cmd),
            &content,
            json!({"status": output.status.code()}),
        )?;
        added += 1;
    }
    if added == 0 {
        bail!(
            "context_capture_failed: provide --text, --stdin, --file, --files, --git-diff, or --cmd"
        );
    }
    Ok(())
}

fn add_file_context(conn: &Connection, context_id: &str, file: &Path) -> Result<()> {
    let content =
        fs::read_to_string(file).with_context(|| format!("read file {}", file.display()))?;
    let metadata = fs::metadata(file)?;
    add_context_item(
        conn,
        context_id,
        "file",
        file.file_name().and_then(|name| name.to_str()),
        Some(&file.display().to_string()),
        &content,
        json!({"path": file.display().to_string(), "size": metadata.len()}),
    )
}

fn add_context_item(
    conn: &Connection,
    context_id: &str,
    kind: &str,
    label: Option<&str>,
    source: Option<&str>,
    content: &str,
    metadata: serde_json::Value,
) -> Result<()> {
    conn.execute(
        "insert into context_items (id, context_id, kind, label, source, content, content_hash, metadata_json, created_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![id(), context_id, kind, label, source, content, content_hash(content), metadata.to_string(), now()],
    )?;
    Ok(())
}

fn context_json(conn: &Connection, context_id: &str) -> Result<serde_json::Value> {
    let header = conn
        .query_row(
            "select c.id, t.name, a.name, c.title, c.created_at
             from contexts c join teams t on t.id=c.team_id join agents a on a.id=c.created_by_agent_id
             where c.id=?1",
            params![context_id],
            |row| Ok(json!({"id": row.get::<_, String>(0)?, "team": row.get::<_, String>(1)?, "created_by": row.get::<_, String>(2)?, "title": row.get::<_, Option<String>>(3)?, "created_at": row.get::<_, String>(4)?})),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown_context: {context_id}"))?;
    let mut stmt = conn.prepare(
        "select id, kind, label, source, content, content_hash, metadata_json, created_at from context_items where context_id=?1 order by created_at asc",
    )?;
    let items = stmt
        .query_map(params![context_id], |row| {
            let metadata: String = row.get(6)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "kind": row.get::<_, String>(1)?,
                "label": row.get::<_, Option<String>>(2)?,
                "source": row.get::<_, Option<String>>(3)?,
                "content": row.get::<_, Option<String>>(4)?,
                "content_hash": row.get::<_, Option<String>>(5)?,
                "metadata": serde_json::from_str::<serde_json::Value>(&metadata).unwrap_or_else(|_| json!({})),
                "created_at": row.get::<_, String>(7)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(json!({"header": header, "items": items}))
}

fn context_plaintext(conn: &Connection, context_id: &str) -> Result<String> {
    let context = context_json(conn, context_id)?;
    let mut output = String::new();
    if let Some(items) = context["items"].as_array() {
        for item in items {
            output.push_str("\n--- ");
            output.push_str(item["kind"].as_str().unwrap_or("context"));
            if let Some(label) = item["label"].as_str() {
                output.push_str(": ");
                output.push_str(label);
            }
            output.push_str(" ---\n");
            output.push_str(item["content"].as_str().unwrap_or_default());
            output.push('\n');
        }
    }
    Ok(output)
}

fn job_row(conn: &Connection, job_id: &str) -> Result<Option<JobRow>> {
    conn.query_row(
        "select j.team_id, t.name, j.thread_id, j.task_message_id, j.context_id, j.requested_by_agent_id, j.target_agent_id, a.name, j.runtime
         from jobs j join teams t on t.id=j.team_id join agents a on a.id=j.target_agent_id
         where j.id=?1",
        params![job_id],
        |row| {
            Ok(JobRow {
                team_id: row.get(0)?,
                team_name: row.get(1)?,
                thread_id: row.get(2)?,
                task_message_id: row.get(3)?,
                context_id: row.get(4)?,
                requested_by_agent_id: row.get(5)?,
                target_agent_id: row.get(6)?,
                target_agent_name: row.get(7)?,
                runtime: row.get(8)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn job_json(conn: &Connection, job_id: &str) -> Result<Option<serde_json::Value>> {
    conn.query_row(
        "select j.id, t.name, j.thread_id, j.task_message_id, j.context_id, ra.name, ta.name, j.runtime, j.state,
                j.retry_of_job_id, j.timeout_seconds, j.process_id, j.created_at, j.started_at, j.finished_at,
                j.result_message_id, j.failure_code, j.failure_message
         from jobs j
         join teams t on t.id=j.team_id
         join agents ra on ra.id=j.requested_by_agent_id
         join agents ta on ta.id=j.target_agent_id
         where j.id=?1",
        params![job_id],
        |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "team": row.get::<_, String>(1)?,
                "thread_id": row.get::<_, String>(2)?,
                "task_message_id": row.get::<_, String>(3)?,
                "context_id": row.get::<_, Option<String>>(4)?,
                "requested_by": row.get::<_, String>(5)?,
                "target": row.get::<_, String>(6)?,
                "runtime": row.get::<_, String>(7)?,
                "state": row.get::<_, String>(8)?,
                "retry_of_job_id": row.get::<_, Option<String>>(9)?,
                "timeout_seconds": row.get::<_, Option<i64>>(10)?,
                "process_id": row.get::<_, Option<i64>>(11)?,
                "created_at": row.get::<_, String>(12)?,
                "started_at": row.get::<_, Option<String>>(13)?,
                "finished_at": row.get::<_, Option<String>>(14)?,
                "result_message_id": row.get::<_, Option<String>>(15)?,
                "failure_code": row.get::<_, Option<String>>(16)?,
                "failure_message": row.get::<_, Option<String>>(17)?,
            }))
        },
    )
    .optional()
    .map_err(Into::into)
}

fn recent_jobs(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare("select id from jobs order by created_at desc limit 20")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|job_id| job_json(conn, &job_id)?.ok_or_else(|| anyhow!("missing job")))
        .collect()
}

fn job_logs(conn: &Connection, job_id: &str, limit: i64) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "select stream, line, created_at from job_logs where job_id=?1 order by created_at desc limit ?2",
    )?;
    let mut logs = stmt
        .query_map(params![job_id, limit], |row| {
            Ok(json!({"stream": row.get::<_, String>(0)?, "line": row.get::<_, String>(1)?, "created_at": row.get::<_, String>(2)?}))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    logs.reverse();
    Ok(logs)
}

fn set_job_state(
    conn: &Connection,
    job_id: &str,
    state: &str,
    failure_code: Option<&str>,
    failure_message: Option<&str>,
) -> Result<()> {
    match state {
        "starting" | "running" => {
            conn.execute(
                "update jobs set state=?1, started_at=coalesce(started_at, ?2), failure_code=?3, failure_message=?4 where id=?5",
                params![state, now(), failure_code, failure_message, job_id],
            )?;
        }
        "blocked" | "failed" | "cancelled" | "timeout" | "succeeded" => {
            conn.execute(
                "update jobs set state=?1, finished_at=coalesce(finished_at, ?2), failure_code=?3, failure_message=?4 where id=?5",
                params![state, now(), failure_code, failure_message, job_id],
            )?;
        }
        _ => bail!("invalid job state: {state}"),
    }
    append_event(
        conn,
        &format!("job.{state}"),
        None,
        None,
        Some(job_id),
        json!({"job_id": job_id, "state": state}),
    )?;
    Ok(())
}

fn append_log(conn: &Connection, job_id: &str, stream: &str, line: &str) -> Result<()> {
    conn.execute(
        "insert into job_logs (id, job_id, stream, line, created_at) values (?1, ?2, ?3, ?4, ?5)",
        params![id(), job_id, stream, line, now()],
    )?;
    append_event(
        conn,
        "job.output",
        None,
        None,
        Some(job_id),
        json!({"stream": stream, "line": line}),
    )?;
    Ok(())
}

fn append_event(
    conn: &Connection,
    event_type: &str,
    team_id: Option<&str>,
    actor_agent_id: Option<&str>,
    subject_id: Option<&str>,
    payload: serde_json::Value,
) -> Result<()> {
    conn.execute(
        "insert into events (id, type, team_id, actor_agent_id, subject_id, payload_json, created_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id(), event_type, team_id, actor_agent_id, subject_id, payload.to_string(), now()],
    )?;
    Ok(())
}

fn adapter_command(runtime: &str, agent_name: &str, task: &str) -> Option<String> {
    let agent_key = format!("HANDOFF_AGENT_CMD_{}", env_key(agent_name));
    if let Ok(command) = env::var(agent_key) {
        return Some(command);
    }
    let runtime_key = format!("HANDOFF_RUNTIME_CMD_{}", env_key(runtime));
    if let Ok(command) = env::var(runtime_key) {
        return Some(command);
    }
    if runtime == "shell" {
        return Some(task.to_string());
    }
    None
}

fn spawn_worker(job_id: &str) -> Result<()> {
    let exe = env::current_exe()?;
    Command::new(exe)
        .arg("worker")
        .arg(job_id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn handoff worker")?;
    Ok(())
}

fn read_message_body(
    message: &[String],
    read_stdin: bool,
    file: Option<&Path>,
    message_text: Option<&str>,
) -> Result<String> {
    let mut sources = 0;
    if !message.is_empty() {
        sources += 1;
    }
    if read_stdin {
        sources += 1;
    }
    if file.is_some() {
        sources += 1;
    }
    if message_text.is_some() {
        sources += 1;
    }
    if sources != 1 {
        bail!("invalid_arguments: provide exactly one message source");
    }
    if !message.is_empty() {
        Ok(message.join(" "))
    } else if read_stdin {
        read_all_stdin()
    } else if let Some(file) = file {
        fs::read_to_string(file).with_context(|| format!("read {}", file.display()))
    } else {
        Ok(message_text.unwrap_or_default().to_string())
    }
}

fn read_all_stdin() -> Result<String> {
    let mut content = String::new();
    io::stdin().read_to_string(&mut content)?;
    Ok(content)
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command]);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.args(["-lc", command]);
        cmd
    }
}

fn validate_mode(runtime: &str, mode: &str) -> Result<()> {
    let supported = match runtime {
        "claude-code" => matches!(mode, "monitor" | "turn" | "both" | "off"),
        "codex" | "gemini" | "copilot" | "antigravity" | "opencode" => {
            matches!(mode, "turn" | "off")
        }
        "shell" | "unknown" => mode == "off",
        _ => false,
    };
    if supported {
        Ok(())
    } else {
        bail!("unsupported_delivery_mode: {runtime} does not support {mode}")
    }
}

fn detect_runtime() -> Runtime {
    if env::var("CODEX_SANDBOX").is_ok() || env::var("CODEX_HOME").is_ok() {
        Runtime::Codex
    } else {
        Runtime::Shell
    }
}

fn project_path(project: Option<PathBuf>) -> Result<String> {
    let path = project.unwrap_or(env::current_dir()?);
    Ok(path.canonicalize().unwrap_or(path).display().to_string())
}

fn id() -> String {
    Uuid::now_v7().to_string()
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn print_json(value: serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(&value).expect("json serialization")
    );
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or("")
}

fn identity_json(identity: &Identity) -> serde_json::Value {
    json!({"team": identity.team, "agent": identity.agent, "runtime": identity.runtime})
}

fn identities_to_json(identities: &[Identity]) -> Vec<serde_json::Value> {
    identities.iter().map(identity_json).collect()
}

fn env_key(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_conn() -> (TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("handoff.db")).unwrap();
        ensure_schema(&conn).unwrap();
        (dir, conn)
    }

    #[test]
    fn join_send_and_inbox_work() {
        let (_dir, conn) = test_conn();
        cmd_join(
            &conn,
            JoinArgs {
                team: "team".into(),
                agent: "alice".into(),
                runtime: Some(Runtime::Shell),
                project: None,
                json: false,
            },
        )
        .unwrap();
        cmd_join(
            &conn,
            JoinArgs {
                team: "team".into(),
                agent: "bob".into(),
                runtime: Some(Runtime::Shell),
                project: None,
                json: false,
            },
        )
        .unwrap();
        cmd_actas(
            &conn,
            AgentArg {
                agent: "alice".into(),
                project: None,
                json: false,
            },
        )
        .unwrap();
        cmd_send(
            &conn,
            SendArgs {
                agent: "bob".into(),
                message: vec!["hello".into()],
                as_agent: None,
                team: None,
                stdin: false,
                file: None,
                subject: None,
                thread: None,
                context: None,
                message_text: None,
                json: false,
            },
            "message",
        )
        .unwrap();
        let bob = agent_by_name(&conn, &get_or_create_team(&conn, "team").unwrap(), "bob").unwrap();
        let inbox = inbox_messages(&conn, &bob.agent_id, true, 10).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0]["body"], "hello");
    }

    #[test]
    fn context_file_capture_hashes_content() {
        let (_dir, conn) = test_conn();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), "context").unwrap();
        cmd_join(
            &conn,
            JoinArgs {
                team: "team".into(),
                agent: "alice".into(),
                runtime: Some(Runtime::Shell),
                project: None,
                json: false,
            },
        )
        .unwrap();
        let identity = resolve_identity(&conn, Some("alice"), None, None).unwrap();
        let context_id = create_context(&conn, &identity, Some("test")).unwrap();
        add_file_context(&conn, &context_id, tmp.path()).unwrap();
        let context = context_json(&conn, &context_id).unwrap();
        assert_eq!(context["items"][0]["kind"], "file");
        assert!(context["items"][0]["content_hash"].as_str().unwrap().len() > 20);
    }
}
