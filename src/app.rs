use crate::delivery::{apply_delivery_mode, print_monitor_instruction, validate_mode};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

const DEFAULT_LIMIT: i64 = 50;
const SCHEMA_VERSION: i64 = 2;
const TERMINAL_JOB_STATE_SQL: &str = "'succeeded','failed','cancelled','timeout','blocked'";

#[derive(Parser, Debug)]
#[command(name = "handoff")]
#[command(about = "Local-first agent messaging, context handoff, and background task runtime")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Initialize local handoff storage")]
    Init,
    #[command(about = "Configure handoff for an agent host in this project")]
    Setup(SetupArgs),
    #[command(about = "Create, configure, and list delegation profiles")]
    Profile(ProfileArgs),
    #[command(about = "List live addressable sessions for this project")]
    Sessions(ProjectArgs),
    #[command(about = "Configure the current live session")]
    Session(SessionArgs),
    #[command(about = "Show the current session")]
    Whoami(ProjectArgs),
    #[command(about = "Show the current session")]
    Active(ProjectArgs),
    #[command(about = "Send a message to another live session")]
    Send(SendArgs),
    #[command(about = "Send a message to another live session")]
    To(SendArgs),
    #[command(about = "Route a message to a capable live session")]
    Route(RouteArgs),
    #[command(about = "Send a message using the peerpost-style alias")]
    Post(SendArgs),
    #[command(about = "Reply to an existing thread")]
    Reply(ReplyArgs),
    #[command(about = "Read the current session inbox")]
    Inbox(InboxArgs),
    #[command(about = "Show recent message history")]
    History(HistoryArgs),
    #[command(about = "Show a message by id")]
    Show(ShowArgs),
    #[command(about = "Configure project-local delivery hooks")]
    Mode(ModeArgs),
    #[command(about = "Write notification files for live sessions")]
    Daemon(DaemonArgs),
    #[command(about = "Consume the current session notification file")]
    Notify(NotifyArgs),
    #[command(about = "Stream unread messages for the current session")]
    Monitor(MonitorArgs),
    #[command(about = "Remove all registrations for this project")]
    Reset(ProjectArgs),
    #[command(about = "Create, show, and list context packages")]
    Context(ContextArgs),
    #[command(about = "Start a background task for a profile")]
    Run(RunArgs),
    #[command(about = "Delegate a task and optionally wait for the result")]
    Delegate(DelegateArgs),
    #[command(about = "Show job status")]
    Status(StatusArgs),
    #[command(about = "Show job logs")]
    Logs(LogsArgs),
    #[command(about = "Show a completed job result")]
    Result(ResultArgs),
    #[command(about = "Cancel a queued or running job")]
    Cancel(JobIdArg),
    #[command(about = "Retry a job")]
    Retry(RetryArgs),
    #[command(about = "Install a symlink alias for this binary")]
    InstallAlias(InstallAliasArgs),
    #[command(hide = true)]
    Worker(JobIdArg),
}

#[derive(Args, Debug)]
struct JsonArgs {
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct ProjectArgs {
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct SetupArgs {
    #[arg(value_enum, help = "Host runtime to configure")]
    runtime: Runtime,
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct ProfileArgs {
    #[command(subcommand)]
    command: ProfileCommand,
}

#[derive(Subcommand, Debug)]
enum ProfileCommand {
    #[command(about = "Create or update a delegation profile")]
    Create(ProfileCreateArgs),
    #[command(about = "List delegation profiles for this project")]
    List(ProjectArgs),
    #[command(about = "Set profile options")]
    Set(ProfileSetArgs),
}

#[derive(Args, Debug)]
struct ProfileCreateArgs {
    #[arg(help = "Profile name")]
    name: String,
    #[arg(long, value_enum, help = "Runtime used by this profile")]
    runtime: Option<Runtime>,
    #[arg(long, help = "Inline system prompt prepended to delegated tasks")]
    prompt: Option<String>,
    #[arg(long, help = "Read the system prompt from this file")]
    prompt_file: Option<PathBuf>,
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct ProfileSetArgs {
    #[arg(help = "Profile name")]
    name: String,
    #[arg(help = "Profile settings in key=value form")]
    settings: Vec<String>,
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct SessionArgs {
    #[command(subcommand)]
    command: SessionCommand,
}

#[derive(Subcommand, Debug)]
enum SessionCommand {
    #[command(about = "Assign an alias to the current session")]
    Alias(SessionAliasArgs),
}

#[derive(Args, Debug)]
struct SessionAliasArgs {
    #[arg(help = "Alias for the current live session")]
    alias: String,
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, value_enum, help = "Runtime for the current session")]
    runtime: Option<Runtime>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug, Clone)]
struct SendArgs {
    #[arg(help = "Recipient session id or @alias")]
    session: String,
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(help = "Message text. Omit when using --stdin, --file, or --message")]
    message: Vec<String>,
    #[arg(long = "as", hide = true)]
    as_agent: Option<String>,
    #[arg(long, hide = true)]
    team: Option<String>,
    #[arg(long, help = "Read message body from standard input")]
    stdin: bool,
    #[arg(
        long,
        help = "Read message body from a file, or attach it as context with --as-context"
    )]
    file: Option<PathBuf>,
    #[arg(
        long,
        help = "Store --file as a context package instead of using it as the message body"
    )]
    as_context: bool,
    #[arg(long, help = "Capture git diff as a context package and attach it")]
    git_diff: bool,
    #[arg(
        long,
        help = "Capture command output as a context package and attach it"
    )]
    cmd: Option<String>,
    #[arg(long, help = "Optional subject for the message or context")]
    subject: Option<String>,
    #[arg(long, help = "Send into an existing thread id")]
    thread: Option<String>,
    #[arg(long, help = "Attach an existing context id")]
    context: Option<String>,
    #[arg(
        long = "message",
        help = "Message text when positional text would be ambiguous"
    )]
    message_text: Option<String>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug, Clone)]
struct RouteArgs {
    #[arg(help = "Message text. Omit to list matching route candidates")]
    message: Vec<String>,
    #[arg(long, help = "Required profile capability, such as review or docs")]
    capability: String,
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, help = "Optional subject for the routed message")]
    subject: Option<String>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct ReplyArgs {
    #[arg(help = "Thread id to reply to")]
    thread_id: String,
    #[arg(help = "Reply text. Omit when using --stdin or --file")]
    message: Vec<String>,
    #[arg(long = "as", hide = true)]
    as_agent: Option<String>,
    #[arg(long, help = "Read reply body from standard input")]
    stdin: bool,
    #[arg(long, help = "Read reply body from a file")]
    file: Option<PathBuf>,
    #[arg(long, help = "Optional subject for the reply")]
    subject: Option<String>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct InboxArgs {
    #[arg(long = "as", hide = true)]
    as_agent: Option<String>,
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, help = "Read inbox for this session id")]
    session_id: Option<String>,
    #[arg(long, help = "Read unread messages only")]
    unread: bool,
    #[arg(long, help = "Include already-read messages")]
    all: bool,
    #[arg(long, help = "Show messages without marking them read")]
    peek: bool,
    #[arg(long, help = "Alias for --peek")]
    no_mark_read: bool,
    #[arg(long, default_value_t = DEFAULT_LIMIT, help = "Maximum number of messages to show")]
    limit: i64,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct HistoryArgs {
    #[arg(long = "with", help = "Filter history to messages with this agent")]
    with_agent: Option<String>,
    #[arg(long, hide = true)]
    team: Option<String>,
    #[arg(long, default_value_t = DEFAULT_LIMIT, help = "Maximum number of messages to show")]
    limit: i64,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct ShowArgs {
    #[arg(help = "Message id to show")]
    message_id: String,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct ModeArgs {
    #[arg(help = "Delivery mode to set. Omit to show the current mode")]
    mode: Option<DeliveryMode>,
    #[arg(long, value_enum, help = "Runtime whose hooks should be configured")]
    runtime: Option<Runtime>,
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct DaemonArgs {
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, default_value_t = 2, help = "Polling interval in seconds")]
    interval: u64,
    #[arg(long, help = "Write notifications once and exit")]
    once: bool,
    #[arg(long, help = "Print machine-readable JSON lines")]
    json: bool,
}

#[derive(Args, Debug)]
struct NotifyArgs {
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, value_enum, help = "Runtime for the current session")]
    runtime: Option<Runtime>,
    #[arg(long, help = "Stable session id")]
    session_id: Option<String>,
    #[arg(long, help = "Emit agent hook response JSON")]
    hook: bool,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct MonitorArgs {
    #[arg(long = "as", hide = true)]
    as_agent: Option<String>,
    #[arg(long, help = "Use this project path instead of the current directory")]
    project: Option<PathBuf>,
    #[arg(long, value_enum, help = "Runtime to monitor")]
    runtime: Option<Runtime>,
    #[arg(long, help = "Stable session id")]
    session_id: Option<String>,
    #[arg(long, default_value_t = 5, help = "Polling interval in seconds")]
    interval: u64,
    #[arg(long, help = "Check once and exit")]
    once: bool,
    #[arg(long, help = "Print machine-readable JSON lines")]
    json: bool,
    #[arg(long, hide = true)]
    instruction: bool,
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
    #[arg(long, help = "Optional title for this context package")]
    title: Option<String>,
    #[arg(long, help = "Add literal text to the context package")]
    text: Option<String>,
    #[arg(long, help = "Read context content from standard input")]
    stdin: bool,
    #[arg(long, help = "Add one file to the context package")]
    file: Option<PathBuf>,
    #[arg(long, help = "Add a file to the context package. Can be repeated")]
    files: Vec<PathBuf>,
    #[arg(long, help = "Capture git diff as context")]
    git_diff: bool,
    #[arg(long, help = "Capture command output as context")]
    cmd: Option<String>,
    #[arg(long = "as", hide = true)]
    as_agent: Option<String>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct ContextShowArgs {
    #[arg(help = "Context id to show")]
    context_id: String,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct RunArgs {
    #[arg(help = "Target profile name")]
    profile: String,
    #[arg(long, help = "Task text or shell command for shell runtime")]
    task: String,
    #[arg(long, help = "Attach an existing context id")]
    context: Option<String>,
    #[arg(long, help = "Capture git diff as context for this task")]
    git_diff: bool,
    #[arg(long, help = "Attach one file as context for this task")]
    file: Option<PathBuf>,
    #[arg(long, help = "Timeout in seconds")]
    timeout: Option<u64>,
    #[arg(long = "as", hide = true)]
    as_agent: Option<String>,
    #[arg(long, help = "Optional subject for the task thread")]
    subject: Option<String>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
    #[arg(skip)]
    retry_of_job_id: Option<String>,
}

#[derive(Args, Debug)]
struct DelegateArgs {
    #[arg(help = "Target profile name")]
    profile: String,
    #[arg(long, help = "Task instructions for the target agent")]
    task: Option<String>,
    #[arg(long, help = "Attach an existing context id")]
    context: Option<String>,
    #[arg(long, help = "Capture git diff as context for this task")]
    git_diff: bool,
    #[arg(long, help = "Read context content from standard input")]
    stdin: bool,
    #[arg(long, help = "Attach one file as context for this task")]
    file: Option<PathBuf>,
    #[arg(long, help = "Timeout in seconds for the delegated job")]
    timeout: Option<u64>,
    #[arg(long, help = "Wait for the job to finish and print the result")]
    wait: bool,
    #[arg(long = "as", hide = true)]
    as_agent: Option<String>,
    #[arg(long, help = "Optional subject for the task thread")]
    subject: Option<String>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct StatusArgs {
    #[arg(help = "Job id to show. Omit to list recent jobs")]
    job_id: Option<String>,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct LogsArgs {
    #[arg(help = "Job id whose logs should be shown")]
    job_id: String,
    #[arg(long, help = "Maximum number of log lines")]
    tail: Option<i64>,
    #[arg(long, help = "Follow new log lines until the job exits")]
    follow: bool,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct ResultArgs {
    #[arg(help = "Job id whose result should be shown")]
    job_id: String,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct JobIdArg {
    #[arg(help = "Job id")]
    job_id: String,
}

#[derive(Args, Debug)]
struct RetryArgs {
    #[arg(help = "Job id to retry")]
    job_id: String,
    #[arg(
        long,
        help = "Retained for compatibility; retry already reuses the original context"
    )]
    same_context: bool,
    #[arg(long, help = "Print machine-readable JSON")]
    json: bool,
}

#[derive(Args, Debug)]
struct InstallAliasArgs {
    #[arg(help = "Alias binary name to install")]
    alias: String,
    #[arg(long, help = "Print machine-readable JSON")]
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

pub fn main() {
    if let Err(error) = run() {
        let message = format!("{error:#}");
        eprintln!("{message}");
        if let Some(hint) = error_hint(&message) {
            eprintln!("\nNext steps:\n{hint}");
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let conn = open_db()?;
    ensure_schema(&conn)?;

    match cli.command.unwrap_or(Commands::Inbox(InboxArgs {
        as_agent: None,
        project: None,
        session_id: None,
        unread: true,
        all: false,
        peek: false,
        no_mark_read: false,
        limit: DEFAULT_LIMIT,
        json: false,
    })) {
        Commands::Init => cmd_init(&conn),
        Commands::Setup(args) => cmd_setup(&conn, args),
        Commands::Profile(args) => cmd_profile(&conn, args),
        Commands::Sessions(args) => cmd_sessions(&conn, args),
        Commands::Session(args) => cmd_session(&conn, args),
        Commands::Whoami(args) => cmd_whoami(&conn, args),
        Commands::Active(args) => cmd_active(&conn, args),
        Commands::Send(args) | Commands::To(args) | Commands::Post(args) => {
            cmd_send(&conn, args, "message")
        }
        Commands::Route(args) => cmd_route(&conn, args),
        Commands::Reply(args) => cmd_reply(&conn, args),
        Commands::Inbox(args) => cmd_inbox(&conn, args),
        Commands::History(args) => cmd_history(&conn, args),
        Commands::Show(args) => cmd_show(&conn, args),
        Commands::Mode(args) => cmd_mode(&conn, args),
        Commands::Daemon(args) => cmd_daemon(&conn, args),
        Commands::Notify(args) => cmd_notify(&conn, args),
        Commands::Monitor(args) => cmd_monitor(&conn, args),
        Commands::Reset(args) => cmd_reset(&conn, args),
        Commands::Context(args) => cmd_context(&conn, args),
        Commands::Run(args) => cmd_run(&conn, args),
        Commands::Delegate(args) => cmd_delegate(&conn, args),
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
    let conn = Connection::open(db_path()?).context("open handoff database")?;
    conn.execute_batch(
        r#"
        pragma foreign_keys = on;
        pragma busy_timeout = 5000;
        "#,
    )?;
    Ok(conn)
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("pragma user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        bail!(
            "unsupported_schema_version: database schema version {version} is newer than this binary supports ({SCHEMA_VERSION})"
        );
    }
    conn.execute_batch(
        r#"
        pragma journal_mode = wal;
        create table if not exists events (
          id text primary key,
          type text not null,
          team_id text references teams(id) on delete set null,
          actor_agent_id text references agents(id) on delete set null,
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
          team_id text not null references teams(id) on delete cascade,
          name text not null,
          runtime text not null,
          created_at text not null,
          updated_at text not null,
          unique(team_id, name)
        );
        create table if not exists project_registrations (
          id text primary key,
          team_id text not null references teams(id) on delete cascade,
          agent_id text not null references agents(id) on delete cascade,
          project_path text not null,
          runtime text not null,
          created_at text not null,
          updated_at text not null,
          unique(team_id, agent_id, project_path, runtime)
        );
        create table if not exists profile_settings (
          agent_id text primary key references agents(id) on delete cascade,
          prompt text,
          prompt_file text,
          options_json text not null default '{}',
          created_at text not null,
          updated_at text not null
        );
        create table if not exists sessions (
          id text primary key,
          team_id text not null references teams(id) on delete cascade,
          agent_id text not null references agents(id) on delete cascade,
          project_path text not null,
          session_key text not null,
          alias text,
          runtime text not null,
          source text not null,
          created_at text not null,
          updated_at text not null,
          last_seen_at text not null,
          unique(project_path, session_key),
          unique(project_path, alias)
        );
        create table if not exists threads (
          id text primary key,
          team_id text not null references teams(id) on delete cascade,
          subject text,
          created_by_agent_id text not null references agents(id) on delete restrict,
          created_at text not null,
          updated_at text not null
        );
        create table if not exists messages (
          id text primary key,
          team_id text not null references teams(id) on delete cascade,
          thread_id text not null references threads(id) on delete cascade,
          from_agent_id text not null references agents(id) on delete restrict,
          to_agent_id text not null references agents(id) on delete restrict,
          kind text not null default 'message',
          context_id text references contexts(id) on delete set null,
          job_id text references jobs(id) on delete set null,
          subject text,
          body text not null,
          created_at text not null
        );
        create table if not exists message_reads (
          message_id text not null references messages(id) on delete cascade,
          agent_id text not null references agents(id) on delete cascade,
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
          team_id text not null references teams(id) on delete cascade,
          agent_id text not null references agents(id) on delete cascade,
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
          team_id text not null references teams(id) on delete cascade,
          created_by_agent_id text not null references agents(id) on delete restrict,
          title text,
          created_at text not null
        );
        create table if not exists context_items (
          id text primary key,
          context_id text not null references contexts(id) on delete cascade,
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
          team_id text not null references teams(id) on delete cascade,
          thread_id text not null references threads(id) on delete cascade,
          task_message_id text not null references messages(id) on delete restrict,
          context_id text references contexts(id) on delete set null,
          requested_by_agent_id text not null references agents(id) on delete restrict,
          target_agent_id text not null references agents(id) on delete restrict,
          runtime text not null,
          state text not null,
          retry_of_job_id text references jobs(id) on delete set null,
          timeout_seconds integer,
          process_id integer,
          created_at text not null,
          started_at text,
          finished_at text,
          result_message_id text references messages(id) on delete set null,
          failure_code text,
          failure_message text
        );
        create table if not exists job_logs (
          id text primary key,
          job_id text not null references jobs(id) on delete cascade,
          stream text not null,
          line text not null,
          created_at text not null
        );
        create index if not exists idx_project_registrations_project on project_registrations(project_path);
        create index if not exists idx_sessions_project on sessions(project_path, updated_at);
        create index if not exists idx_sessions_agent on sessions(agent_id);
        create index if not exists idx_profile_settings_agent on profile_settings(agent_id);
        create index if not exists idx_messages_inbox_unread on messages(to_agent_id, created_at);
        create index if not exists idx_messages_thread on messages(thread_id, created_at);
        create index if not exists idx_messages_team_history on messages(team_id, created_at);
        create index if not exists idx_message_reads_agent on message_reads(agent_id, message_id);
        create index if not exists idx_context_items_context on context_items(context_id, created_at);
        create index if not exists idx_jobs_state_created on jobs(state, created_at);
        create index if not exists idx_jobs_target on jobs(target_agent_id, created_at);
        create index if not exists idx_job_logs_job on job_logs(job_id, created_at);
        "#,
    )?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

fn cmd_init(_conn: &Connection) -> Result<()> {
    println!("Initialized handoff at {}", app_home()?.display());
    println!("Database: {}", db_path()?.display());
    Ok(())
}

fn cmd_setup(conn: &Connection, args: SetupArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let runtime = args.runtime.as_str();
    let session = current_session(conn, &project, Some(runtime), None)?;
    let mode = if runtime == "claude-code" {
        "both"
    } else {
        "turn"
    };
    validate_mode(runtime, mode)?;
    apply_delivery_mode(&project, runtime, mode)?;
    write_mcp_config(&project)?;
    if runtime == "claude-code" {
        install_handoff_skill(&project)?;
    }
    if args.json {
        print_json(
            json!({"ok": true, "project": project, "runtime": runtime, "session": identity_json(&session), "mode": mode}),
        );
    } else {
        println!("Initialized handoff at {}", app_home()?.display());
        println!("session: {} ({})", session.agent, session.runtime);
        println!("delivery: {mode}");
        println!("mcp: .mcp.json");
        if runtime == "claude-code" {
            println!("skill: .claude/skills/handoff/SKILL.md");
        }
    }
    Ok(())
}

fn cmd_profile(conn: &Connection, args: ProfileArgs) -> Result<()> {
    match args.command {
        ProfileCommand::Create(args) => cmd_profile_create(conn, args),
        ProfileCommand::List(args) => cmd_profile_list(conn, args),
        ProfileCommand::Set(args) => cmd_profile_set(conn, args),
    }
}

fn cmd_profile_create(conn: &Connection, args: ProfileCreateArgs) -> Result<()> {
    if args.prompt.is_some() && args.prompt_file.is_some() {
        bail!("invalid_arguments: use either --prompt or --prompt-file, not both");
    }
    let runtime = args.runtime.unwrap_or_else(detect_runtime);
    let project = project_path(args.project)?;
    let team_id = project_team_id(conn, &project)?;
    let agent_id = get_or_create_agent(conn, &team_id, &args.name, runtime.as_str())?;
    register_project_agent(conn, &team_id, &agent_id, &project, runtime.as_str())?;
    upsert_profile_settings(
        conn,
        &agent_id,
        args.prompt.as_deref(),
        args.prompt_file.as_deref(),
        None,
    )?;
    append_event(
        conn,
        "profile.created",
        Some(&team_id),
        Some(&agent_id),
        Some(&agent_id),
        json!({"project": project, "profile": args.name, "runtime": runtime.as_str()}),
    )?;
    if args.json {
        print_json(
            json!({"ok": true, "profile": args.name, "runtime": runtime.as_str(), "project": project}),
        );
    } else {
        println!("profile {} ({})", args.name, runtime.as_str());
    }
    Ok(())
}

fn cmd_profile_list(conn: &Connection, args: ProjectArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let profiles = list_profiles(conn, &project)?;
    if args.json {
        print_json(json!({"ok": true, "project": project, "profiles": profiles}));
    } else if profiles.is_empty() {
        println!("No profiles");
    } else {
        for profile in profiles {
            println!(
                "{} ({})",
                profile["name"].as_str().unwrap_or_default(),
                profile["runtime"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn cmd_profile_set(conn: &Connection, args: ProfileSetArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let profile = profile_by_name(conn, &project, &args.name)?;
    let mut options = profile_options(conn, &profile.agent_id)?;
    let mut prompt = None;
    let mut prompt_file = None;
    for setting in &args.settings {
        let Some((key, value)) = setting.split_once('=') else {
            bail!("invalid_arguments: expected key=value setting, got {setting}");
        };
        match key {
            "prompt" => prompt = Some(value.to_string()),
            "prompt_file" | "prompt-file" => prompt_file = Some(PathBuf::from(value)),
            "runtime" => {
                conn.execute(
                    "update agents set runtime=?1, updated_at=?2 where id=?3",
                    params![value, now(), profile.agent_id],
                )?;
                register_project_agent(conn, &profile.team_id, &profile.agent_id, &project, value)?;
            }
            _ => {
                options[key] = Value::String(value.to_string());
            }
        }
    }
    upsert_profile_settings(
        conn,
        &profile.agent_id,
        prompt.as_deref(),
        prompt_file.as_deref(),
        Some(options),
    )?;
    if args.json {
        print_json(json!({"ok": true, "profile": args.name}));
    } else {
        println!("updated profile {}", args.name);
    }
    Ok(())
}

fn cmd_sessions(conn: &Connection, args: ProjectArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let current = current_session(conn, &project, None, None)?;
    let sessions = list_sessions(conn, &project)?;
    if args.json {
        print_json(
            json!({"ok": true, "project": project, "current": identity_json(&current), "sessions": sessions}),
        );
    } else {
        for session in sessions {
            let marker = if session["agent_id"].as_str() == Some(current.agent_id.as_str()) {
                "*"
            } else {
                " "
            };
            let alias = session["alias"].as_str().unwrap_or("");
            let address = session["address"].as_str().unwrap_or_default();
            let reachability = session["reachability"].as_str().unwrap_or_default();
            println!(
                "{marker} {} {} ({}) {}",
                session["session_key"].as_str().unwrap_or_default(),
                if address.is_empty() { alias } else { address },
                session["runtime"].as_str().unwrap_or_default(),
                reachability
            );
        }
    }
    Ok(())
}

fn cmd_session(conn: &Connection, args: SessionArgs) -> Result<()> {
    match args.command {
        SessionCommand::Alias(args) => cmd_session_alias(conn, args),
    }
}

fn cmd_session_alias(conn: &Connection, args: SessionAliasArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let runtime = args.runtime.unwrap_or_else(detect_runtime);
    let session = current_session(conn, &project, Some(runtime.as_str()), None)?;
    let alias = normalize_session_alias(&args.alias)?;
    set_session_alias(conn, &project, &session.agent_id, &alias)?;
    if args.json {
        print_json(
            json!({"ok": true, "session": session.agent, "alias": alias, "address": format!("@{alias}")}),
        );
    } else {
        println!("session alias @{alias}");
    }
    Ok(())
}

fn cmd_whoami(conn: &Connection, args: ProjectArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let session = current_session(conn, &project, None, None)?;
    if args.json {
        print_json(json!({"ok": true, "project": project, "session": identity_json(&session)}));
    } else {
        println!("{} ({})", session.agent, session.runtime);
    }
    Ok(())
}

fn cmd_active(conn: &Connection, args: ProjectArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let active = current_session(conn, &project, None, None)?;
    if args.json {
        print_json(json!({"ok": true, "active": identity_json(&active)}));
    } else {
        println!("{} ({})", active.agent, active.runtime);
    }
    Ok(())
}

fn cmd_send(conn: &Connection, args: SendArgs, default_kind: &str) -> Result<()> {
    let project = project_path(args.project.clone())?;
    let sender = sender_session(conn, &project, args.as_agent.as_deref())?;
    let recipient = recipient_session_for_send(conn, &project, &args.session)?;
    let created_context = create_send_context_if_requested(conn, &sender, &args)?;
    let context_id = args.context.as_deref().or(created_context.as_deref());
    let body = read_send_body(&args, context_id.is_some())?;
    let kind = if context_id.is_some() {
        "context"
    } else {
        default_kind
    };
    let message_id = create_message(
        conn,
        NewMessage {
            sender: &sender,
            recipient_agent_id: &recipient.agent_id,
            thread_id: args.thread.as_deref(),
            kind,
            context_id,
            job_id: None,
            subject: args.subject.as_deref(),
            body: &body,
        },
    )?;
    if let Some(context_id) = context_id {
        append_event(
            conn,
            "context.attached",
            Some(&sender.team_id),
            Some(&sender.agent_id),
            Some(context_id),
            json!({"message_id": message_id, "context_id": context_id}),
        )?;
    }
    if args.json {
        print_json(json!({"ok": true, "message_id": message_id}));
    } else {
        println!("sent {message_id}");
    }
    Ok(())
}

fn cmd_route(conn: &Connection, args: RouteArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let candidates = route_candidates(conn, &project, &args.capability)?;
    if args.message.is_empty() {
        if args.json {
            print_json(
                json!({"ok": true, "project": project, "capability": args.capability, "candidates": candidates}),
            );
        } else if candidates.is_empty() {
            println!("No route candidates");
        } else {
            for candidate in candidates {
                println!(
                    "{} -> {} [{}] {}",
                    candidate["profile"].as_str().unwrap_or_default(),
                    candidate["session"].as_str().unwrap_or_default(),
                    candidate["capability"].as_str().unwrap_or_default(),
                    candidate["reachability"].as_str().unwrap_or_default()
                );
            }
        }
        return Ok(());
    }
    let sendable = candidates
        .iter()
        .filter(|candidate| candidate["reachability"].as_str() == Some("active"))
        .collect::<Vec<_>>();
    if sendable.is_empty() {
        bail!(
            "route_not_found: no profile with capability '{}' has an active session",
            args.capability
        );
    }
    if sendable.len() > 1 {
        bail!(
            "ambiguous_route: capability '{}' matches {} sessions; inspect with `handoff route --capability {}` and send to @alias explicitly",
            args.capability,
            sendable.len(),
            args.capability
        );
    }
    let candidate = sendable[0];
    let session = candidate["session"]
        .as_str()
        .ok_or_else(|| anyhow!("route candidate missing session"))?;
    let send_args = SendArgs {
        session: session.to_string(),
        project: Some(PathBuf::from(&project)),
        message: args.message,
        as_agent: None,
        team: None,
        stdin: false,
        file: None,
        as_context: false,
        git_diff: false,
        cmd: None,
        subject: args.subject,
        thread: None,
        context: None,
        message_text: None,
        json: args.json,
    };
    cmd_send(conn, send_args, "message")
}

fn cmd_reply(conn: &Connection, args: ReplyArgs) -> Result<()> {
    let body = read_message_body(&args.message, args.stdin, args.file.as_deref(), None)?;
    let project = project_path(None)?;
    let sender = sender_session(conn, &project, args.as_agent.as_deref())?;
    let recipient_id = reply_recipient(conn, &args.thread_id, &sender.agent_id)?;
    let message_id = create_message(
        conn,
        NewMessage {
            sender: &sender,
            recipient_agent_id: &recipient_id,
            thread_id: Some(&args.thread_id),
            kind: "message",
            context_id: None,
            job_id: None,
            subject: args.subject.as_deref(),
            body: &body,
        },
    )?;
    if args.json {
        print_json(json!({"ok": true, "message_id": message_id, "thread_id": args.thread_id}));
    } else {
        println!("replied {message_id}");
    }
    Ok(())
}

fn cmd_inbox(conn: &Connection, args: InboxArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let identity = current_session(conn, &project, None, args.session_id.as_deref())?;
    let unread_only = !args.all;
    let messages = inbox_messages(conn, &identity.agent_id, unread_only, args.limit)?;
    let mark_read = !(args.peek || args.no_mark_read);
    if mark_read {
        for message in &messages {
            let message_id = message["id"].as_str().unwrap_or_default();
            mark_message_read(conn, &identity, message_id)?;
        }
    }
    if args.json {
        print_json(json!({"ok": true, "messages": messages, "marked_read": mark_read}));
    } else if messages.is_empty() {
        if mark_read {
            println!("No messages");
        } else {
            println!("No messages (peek; nothing marked read)");
        }
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
        if !mark_read {
            println!("peek mode: messages were not marked read");
        }
    }
    Ok(())
}

fn cmd_history(conn: &Connection, args: HistoryArgs) -> Result<()> {
    let project = project_path(None)?;
    let identity = current_session(conn, &project, None, None)?;
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
        apply_delivery_mode(&project, runtime.as_str(), mode.as_str())?;
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

fn cmd_daemon(conn: &Connection, args: DaemonArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let interval = args.interval.max(1);
    loop {
        let written = write_notify_files(conn, &project)?;
        if args.json {
            print_json(json!({"ok": true, "project": project, "written": written}));
        }
        if args.once {
            if !args.json {
                println!("wrote {written} notification file(s)");
            }
            return Ok(());
        }
        thread::sleep(Duration::from_secs(interval));
    }
}

fn cmd_notify(conn: &Connection, args: NotifyArgs) -> Result<()> {
    let project = project_path(args.project)?;
    let runtime = args.runtime.unwrap_or_else(detect_runtime);
    let session_key = current_session_key(conn, &project, args.session_id)?;
    let identity = current_session(conn, &project, Some(runtime.as_str()), Some(&session_key))?;
    let path = notify_path(&project, &session_key)?;
    if !path.exists() {
        let messages = inbox_messages(conn, &identity.agent_id, true, DEFAULT_LIMIT)?;
        for message in &messages {
            let message_id = message["id"].as_str().unwrap_or_default();
            mark_message_read(conn, &identity, message_id)?;
        }
        if args.json {
            if args.hook {
                let body = if messages.is_empty() {
                    String::new()
                } else {
                    let session = NotifySession {
                        agent_id: identity.agent_id.clone(),
                        session_key: session_key.clone(),
                        label: identity.agent.clone(),
                    };
                    notify_markdown(&session, &messages)
                };
                print_json(hook_response_json(&body));
            } else {
                print_json(
                    json!({"ok": true, "messages": messages, "marked_read": true, "source": "inbox"}),
                );
            }
        } else if messages.is_empty() {
            println!("No notifications");
        } else {
            let session = NotifySession {
                agent_id: identity.agent_id,
                session_key,
                label: identity.agent,
            };
            print!("{}", notify_markdown(&session, &messages));
        }
        return Ok(());
    }
    let content = fs::read_to_string(&path)?;
    let message_ids = notify_message_ids(&content);
    for message_id in &message_ids {
        mark_message_read(conn, &identity, message_id)?;
    }
    let _ = fs::remove_file(&path);
    if args.json {
        if args.hook {
            print_json(hook_response_json(&content));
        } else {
            print_json(json!({"ok": true, "message_ids": message_ids, "body": content}));
        }
    } else {
        print!("{content}");
    }
    Ok(())
}

fn cmd_monitor(conn: &Connection, args: MonitorArgs) -> Result<()> {
    let project = project_path(args.project.clone())?;
    let runtime = args.runtime.unwrap_or_else(detect_runtime);
    if args.instruction {
        let session_id = current_session_key(
            conn,
            &project,
            args.session_id
                .or_else(|| session_id_from_hook_stdin().ok().flatten()),
        )?;
        print_monitor_instruction(&project, runtime.as_str(), &session_id)?;
        return Ok(());
    }

    let session_id = current_session_key(conn, &project, args.session_id)?;
    claim_monitor_pid(&session_id)?;

    let interval = args.interval.max(1);
    loop {
        let identity = current_session(conn, &project, Some(runtime.as_str()), Some(&session_id))?;
        let identities = vec![identity];
        let delivered = deliver_monitor_messages(conn, &identities, args.json)?;
        if args.once {
            if args.json && delivered == 0 {
                print_json(json!({"ok": true, "messages": []}));
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(interval));
    }
}

fn cmd_reset(conn: &Connection, args: ProjectArgs) -> Result<()> {
    let project = project_path(args.project)?;
    conn.execute(
        "delete from sessions where project_path=?1",
        params![project],
    )?;
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

fn cmd_context(conn: &Connection, args: ContextArgs) -> Result<()> {
    match args.command {
        ContextCommand::Create(args) => cmd_context_create(conn, args),
        ContextCommand::Show(args) => cmd_context_show(conn, args),
        ContextCommand::List(args) => cmd_context_list(conn, args),
    }
}

fn cmd_context_create(conn: &Connection, args: ContextCreateArgs) -> Result<()> {
    let project = project_path(None)?;
    let identity = sender_session(conn, &project, args.as_agent.as_deref())?;
    let context_id = create_context(conn, &identity, args.title.as_deref())?;
    add_context_inputs(
        conn,
        &context_id,
        ContextInputSpec {
            text: args.text.as_deref(),
            stdin: args.stdin,
            file: args.file.as_deref(),
            files: &args.files,
            git_diff: args.git_diff,
            cmd: args.cmd.as_deref(),
        },
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
    let project = project_path(None)?;
    let identity = current_session(conn, &project, None, None)?;
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
    let project = project_path(None)?;
    let sender = sender_session(conn, &project, args.as_agent.as_deref())?;
    let target = profile_by_name(conn, &project, &args.profile)?;
    let job_id = create_run_job(
        conn,
        &sender,
        &target,
        RunJobSpec {
            task: &args.task,
            context_id: args.context.as_deref(),
            context_inputs: Some(ContextInputSpec {
                text: None,
                stdin: false,
                file: args.file.as_deref(),
                files: &[],
                git_diff: args.git_diff,
                cmd: None,
            }),
            subject: args.subject.as_deref(),
            timeout: args.timeout,
            retry_of_job_id: args.retry_of_job_id.as_deref(),
        },
    )?;
    spawn_worker(&job_id)?;
    if args.json {
        print_json(json!({"ok": true, "job_id": job_id}));
    } else {
        println!("job {job_id}");
    }
    Ok(())
}

fn cmd_delegate(conn: &Connection, args: DelegateArgs) -> Result<()> {
    let project = project_path(None)?;
    let sender = sender_session(conn, &project, args.as_agent.as_deref())?;
    let target = profile_by_name(conn, &project, &args.profile)?;
    let task = match (args.task.as_deref(), target.runtime.as_str()) {
        (Some(task), _) => task,
        (None, "shell") => {
            bail!("invalid_arguments: --task is required when delegating to a shell runtime");
        }
        (None, _) => "Complete the delegated task using the attached context.",
    };
    let job_id = create_run_job(
        conn,
        &sender,
        &target,
        RunJobSpec {
            task,
            context_id: args.context.as_deref(),
            context_inputs: Some(ContextInputSpec {
                text: None,
                stdin: args.stdin,
                file: args.file.as_deref(),
                files: &[],
                git_diff: args.git_diff,
                cmd: None,
            }),
            subject: args.subject.as_deref(),
            timeout: args.timeout,
            retry_of_job_id: None,
        },
    )?;
    spawn_worker(&job_id)?;
    if args.wait {
        let result = wait_for_job_result(conn, &job_id)?;
        if args.json {
            print_json(json!({"ok": true, "job_id": job_id, "result": result}));
        } else {
            println!("{}", result["body"].as_str().unwrap_or_default());
        }
    } else if args.json {
        print_json(json!({"ok": true, "job_id": job_id}));
    } else {
        println!("job {job_id}");
    }
    Ok(())
}

struct RunJobSpec<'a> {
    task: &'a str,
    context_id: Option<&'a str>,
    context_inputs: Option<ContextInputSpec<'a>>,
    subject: Option<&'a str>,
    timeout: Option<u64>,
    retry_of_job_id: Option<&'a str>,
}

fn create_run_job(
    conn: &Connection,
    sender: &Identity,
    target: &AgentRef,
    spec: RunJobSpec<'_>,
) -> Result<String> {
    if spec.context_id.is_some()
        && spec
            .context_inputs
            .as_ref()
            .is_some_and(ContextInputSpec::has_inputs)
    {
        bail!("invalid_arguments: use either --context or context capture flags, not both");
    }
    let captured_context_items = if spec.context_id.is_none() {
        spec.context_inputs
            .filter(ContextInputSpec::has_inputs)
            .map(capture_context_inputs)
            .transpose()?
    } else {
        None
    };
    let tx = conn.unchecked_transaction()?;
    let context_id = if let Some(context_id) = spec.context_id {
        Some(context_id.to_string())
    } else if let Some(items) = captured_context_items {
        let created = create_context(&tx, sender, spec.subject)?;
        persist_context_items(&tx, &created, &items)?;
        Some(created)
    } else {
        None
    };
    let task_message_id = create_message_in_tx(
        &tx,
        NewMessage {
            sender,
            recipient_agent_id: &target.agent_id,
            thread_id: None,
            kind: "task",
            context_id: context_id.as_deref(),
            job_id: None,
            subject: spec.subject,
            body: spec.task,
        },
    )?;
    let thread_id = message_thread(&tx, &task_message_id)?;
    let job_id = id();
    tx.execute(
        "insert into jobs (id, team_id, thread_id, task_message_id, context_id, requested_by_agent_id, target_agent_id, runtime, state, retry_of_job_id, timeout_seconds, created_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', ?9, ?10, ?11)",
        params![
            job_id,
            sender.team_id,
            thread_id,
            task_message_id,
            context_id,
            sender.agent_id,
            target.agent_id,
            target.runtime,
            spec.retry_of_job_id,
            spec.timeout.map(|value| value as i64),
            now()
        ],
    )?;
    tx.execute(
        "update messages set job_id=?1 where id=?2",
        params![job_id, task_message_id],
    )?;
    append_event(
        &tx,
        "job.created",
        Some(&sender.team_id),
        Some(&sender.agent_id),
        Some(&job_id),
        json!({"job_id": job_id}),
    )?;
    tx.commit()?;
    Ok(job_id)
}

fn wait_for_job_result(conn: &Connection, job_id: &str) -> Result<serde_json::Value> {
    loop {
        let job = job_json(conn, job_id)?.ok_or_else(|| anyhow!("unknown job: {job_id}"))?;
        let state = job["state"].as_str().unwrap_or_default();
        if state == "succeeded" {
            let message_id = job["result_message_id"]
                .as_str()
                .ok_or_else(|| anyhow!("job_succeeded_without_result: {job_id}"))?;
            return message_json(conn, message_id)?
                .ok_or_else(|| anyhow!("missing result message: {message_id}"));
        }
        if is_terminal_job_state(state) {
            let failure = job["failure_message"]
                .as_str()
                .or_else(|| job["failure_code"].as_str())
                .unwrap_or("job finished without a result");
            bail!("delegate_failed: {state}: {failure}");
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn cmd_status(conn: &Connection, args: StatusArgs) -> Result<()> {
    if let Some(job_id) = args.job_id {
        let status = job_json(conn, &job_id)?.ok_or_else(|| anyhow!("unknown job: {job_id}"))?;
        if args.json {
            print_json(json!({"ok": true, "job": status}));
        } else {
            print_job_status(&status);
        }
    } else {
        let jobs = recent_jobs(conn)?;
        if args.json {
            print_json(json!({"ok": true, "jobs": jobs}));
        } else {
            if jobs.is_empty() {
                println!("No jobs");
            } else {
                for job in jobs {
                    println!(
                        "[{}] {} -> {} ({}) {}",
                        job["id"].as_str().unwrap_or_default(),
                        job["requested_by"].as_str().unwrap_or_default(),
                        job["target"].as_str().unwrap_or_default(),
                        job["runtime"].as_str().unwrap_or_default(),
                        job["state"].as_str().unwrap_or_default()
                    );
                }
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
        let mut last_created_at = None;
        for log in &logs {
            print_log_line(log);
            last_created_at = log["created_at"].as_str().map(ToOwned::to_owned);
        }
        if args.follow {
            follow_job_logs(conn, &args.job_id, last_created_at.as_deref())?;
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
        bail!(
            "job_not_finished: job has no result yet\nRun: handoff status {} && handoff logs {}",
            args.job_id,
            args.job_id
        );
    }
}

fn cmd_cancel(conn: &Connection, job_id: &str) -> Result<()> {
    let job = job_json(conn, job_id)?.ok_or_else(|| anyhow!("unknown job: {job_id}"))?;
    let state = job["state"].as_str().unwrap_or_default();
    if is_terminal_job_state(state) {
        println!("job already finished: {state}");
        return Ok(());
    }
    if let Some(pid) = job["process_id"].as_i64() {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    if set_job_state(conn, job_id, "cancelled", None, None)? {
        println!("cancelled {job_id}");
    } else {
        println!("job already finished: {}", job_state(conn, job_id)?);
    }
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
        profile: target,
        task: task["body"].as_str().unwrap_or_default().to_string(),
        context: job["context_id"].as_str().map(ToOwned::to_owned),
        git_diff: false,
        file: None,
        timeout: job["timeout_seconds"].as_i64().map(|value| value as u64),
        as_agent: None,
        subject: task["subject"].as_str().map(ToOwned::to_owned),
        json: args.json,
        retry_of_job_id: Some(args.job_id),
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

fn write_mcp_config(project: &str) -> Result<()> {
    let path = Path::new(project).join(".mcp.json");
    let mut value = if path.exists() {
        let content = fs::read_to_string(&path)?;
        serde_json::from_str::<Value>(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    if !value.is_object() {
        value = json!({});
    }
    let object = value.as_object_mut().expect("object initialized");
    let servers = object
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !servers.is_object() {
        *servers = Value::Object(serde_json::Map::new());
    }
    let exe = env::current_exe()?;
    let server_path = mcp_server_path()?;
    let server = if let Some(server_path) = server_path {
        json!({
            "command": "node",
            "args": [server_path.display().to_string()],
            "env": {"HANDOFF_BIN": exe.display().to_string()}
        })
    } else {
        json!({
            "command": "agent-handoff-mcp",
            "env": {"HANDOFF_BIN": exe.display().to_string()}
        })
    };
    servers
        .as_object_mut()
        .expect("mcpServers object initialized")
        .insert("agent-handoff".to_string(), server);
    fs::write(path, serde_json::to_string_pretty(&value)? + "\n")?;
    Ok(())
}

fn mcp_server_path() -> Result<Option<PathBuf>> {
    let cwd = env::current_dir()?;
    let candidate = cwd
        .join("integrations")
        .join("mcp")
        .join("agent-handoff-mcp")
        .join("server.js");
    if candidate.exists() {
        Ok(Some(candidate))
    } else {
        Ok(None)
    }
}

fn install_handoff_skill(project: &str) -> Result<()> {
    let source = Path::new(project)
        .join("integrations")
        .join("skills")
        .join("agent-handoff")
        .join("SKILL.md");
    let content = if source.exists() {
        fs::read_to_string(source)?
    } else {
        default_skill_content().to_string()
    };
    let target = Path::new(project)
        .join(".claude")
        .join("skills")
        .join("handoff")
        .join("SKILL.md");
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, content)?;
    Ok(())
}

fn default_skill_content() -> &'static str {
    "# handoff\n\nUse `handoff delegate <profile> --task \"...\" --wait` for synchronous sub-agent work.\nUse `handoff to @<alias> \"...\"` for live session coordination.\nUse `handoff route --capability <capability> \"...\"` after linking profiles with `profile set <profile> session=@alias capability=...`.\nRun `handoff notify` after turns when notification hooks are unavailable.\n"
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
    let adapter = adapter_command(&job.runtime, &job.target_agent_name, &task_body);
    let Some(adapter) = adapter else {
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
    let system_prompt = profile_system_prompt(conn, &job.target_agent_id)?;
    let prompt = agent_prompt(system_prompt.as_deref(), &task_body, &context_text);
    append_log(
        conn,
        job_id,
        "adapter",
        &format!("running: {}", adapter.command),
    )?;
    let mut child = shell_command(&adapter.command)
        .env("HANDOFF_JOB_ID", job_id)
        .env("HANDOFF_TASK", &task_body)
        .env("HANDOFF_CONTEXT", &context_text)
        .env("HANDOFF_PROMPT", &prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn adapter command")?;
    conn.execute(
        "update jobs set process_id=?1 where id=?2",
        params![child.id() as i64, job_id],
    )?;
    let output = wait_for_child_with_logs(conn, job_id, &mut child, job.timeout_seconds)?;
    if output.timed_out {
        let timeout_seconds = job.timeout_seconds.unwrap_or_default();
        if set_job_state(
            conn,
            job_id,
            "timeout",
            Some("timeout"),
            Some(&format!("Job exceeded timeout of {timeout_seconds}s")),
        )? {
            append_event(
                conn,
                "job.failed",
                Some(&job.team_id),
                Some(&job.target_agent_id),
                Some(job_id),
                json!({"job_id": job_id, "state": "timeout"}),
            )?;
        }
        return Ok(());
    }
    if is_terminal_job_state(&job_state(conn, job_id)?) {
        return Ok(());
    }
    let stdout = output.stdout;
    let stderr = output.stderr;
    if output.status.success() {
        let result_body = if stdout.trim().is_empty() {
            "Task completed with no output.".to_string()
        } else {
            adapter_result_body(&adapter, &stdout)
        };
        let target_identity = Identity {
            team_id: job.team_id.clone(),
            team: job.team_name.clone(),
            agent_id: job.target_agent_id.clone(),
            agent: job.target_agent_name.clone(),
            runtime: job.runtime.clone(),
        };
        let tx = conn.unchecked_transaction()?;
        if is_terminal_job_state(&job_state(&tx, job_id)?) {
            tx.commit()?;
            return Ok(());
        }
        let result_message_id = create_message_in_tx(
            &tx,
            NewMessage {
                sender: &target_identity,
                recipient_agent_id: &job.requested_by_agent_id,
                thread_id: Some(&job.thread_id),
                kind: "result",
                context_id: job.context_id.as_deref(),
                job_id: Some(job_id),
                subject: Some("Task result"),
                body: &result_body,
            },
        )?;
        let update_sql = format!(
            "update jobs set state='succeeded', finished_at=?1, result_message_id=?2, process_id=null where id=?3 and state not in ({TERMINAL_JOB_STATE_SQL})"
        );
        let updated = tx.execute(&update_sql, params![now(), result_message_id, job_id])?;
        if updated == 0 {
            tx.rollback()?;
            return Ok(());
        }
        append_event(
            &tx,
            "job.completed",
            Some(&job.team_id),
            Some(&job.target_agent_id),
            Some(job_id),
            json!({"job_id": job_id}),
        )?;
        tx.commit()?;
    } else {
        if set_job_state(
            conn,
            job_id,
            "failed",
            Some("adapter_failed"),
            Some(&stderr),
        )? {
            append_event(
                conn,
                "job.failed",
                Some(&job.team_id),
                Some(&job.target_agent_id),
                Some(job_id),
                json!({"job_id": job_id}),
            )?;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct AgentRef {
    team_id: String,
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
    timeout_seconds: Option<i64>,
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

fn project_team_id(conn: &Connection, project: &str) -> Result<String> {
    let basename = Path::new(project)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    let hash = content_hash(project);
    let name = format!("project:{basename}:{}", &hash[..12]);
    get_or_create_team(conn, &name)
}

fn register_project_agent(
    conn: &Connection,
    team_id: &str,
    agent_id: &str,
    project: &str,
    runtime: &str,
) -> Result<()> {
    conn.execute(
        "insert into project_registrations (id, team_id, agent_id, project_path, runtime, created_at, updated_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         on conflict(team_id, agent_id, project_path, runtime)
         do update set updated_at=excluded.updated_at",
        params![id(), team_id, agent_id, project, runtime, now()],
    )?;
    Ok(())
}

fn upsert_profile_settings(
    conn: &Connection,
    agent_id: &str,
    prompt: Option<&str>,
    prompt_file: Option<&Path>,
    options: Option<Value>,
) -> Result<()> {
    let existing = conn
        .query_row(
            "select prompt, prompt_file, options_json from profile_settings where agent_id=?1",
            params![agent_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let (current_prompt, current_prompt_file, current_options) =
        existing.unwrap_or((None, None, "{}".to_string()));
    let options_json = options
        .unwrap_or_else(|| serde_json::from_str(&current_options).unwrap_or_else(|_| json!({})));
    conn.execute(
        "insert into profile_settings (agent_id, prompt, prompt_file, options_json, created_at, updated_at)
         values (?1, ?2, ?3, ?4, ?5, ?5)
         on conflict(agent_id) do update set
           prompt=excluded.prompt,
           prompt_file=excluded.prompt_file,
           options_json=excluded.options_json,
           updated_at=excluded.updated_at",
        params![
            agent_id,
            prompt.map(ToOwned::to_owned).or(current_prompt),
            prompt_file
                .map(|path| path.display().to_string())
                .or(current_prompt_file),
            options_json.to_string(),
            now()
        ],
    )?;
    Ok(())
}

fn profile_options(conn: &Connection, agent_id: &str) -> Result<Value> {
    let options_json: Option<String> = conn
        .query_row(
            "select options_json from profile_settings where agent_id=?1",
            params![agent_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(options_json
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_else(|| json!({})))
}

fn profile_system_prompt(conn: &Connection, agent_id: &str) -> Result<Option<String>> {
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "select prompt, prompt_file from profile_settings where agent_id=?1",
            params![agent_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((prompt, prompt_file)) = row else {
        return Ok(None);
    };
    if let Some(prompt) = prompt.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(prompt));
    }
    if let Some(prompt_file) = prompt_file.filter(|value| !value.trim().is_empty()) {
        return fs::read_to_string(&prompt_file)
            .with_context(|| format!("read profile prompt file {prompt_file}"))
            .map(Some);
    }
    Ok(None)
}

fn list_profiles(conn: &Connection, project: &str) -> Result<Vec<Value>> {
    let team_id = project_team_id(conn, project)?;
    let mut stmt = conn.prepare(
        "select a.name, a.runtime, ps.prompt, ps.prompt_file, ps.options_json, a.created_at
         from agents a
         join profile_settings ps on ps.agent_id=a.id
         join project_registrations pr on pr.agent_id=a.id and pr.team_id=a.team_id
         where a.team_id=?1 and pr.project_path=?2
         order by a.name",
    )?;
    let rows = stmt.query_map(params![team_id, project], |row| {
        let options_json: String = row.get(4)?;
        Ok(json!({
            "name": row.get::<_, String>(0)?,
            "runtime": row.get::<_, String>(1)?,
            "prompt": row.get::<_, Option<String>>(2)?,
            "prompt_file": row.get::<_, Option<String>>(3)?,
            "options": serde_json::from_str::<Value>(&options_json).unwrap_or_else(|_| json!({})),
            "created_at": row.get::<_, String>(5)?,
        }))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn profile_by_name(conn: &Connection, project: &str, name: &str) -> Result<AgentRef> {
    let team_id = project_team_id(conn, project)?;
    conn.query_row(
        "select a.id, a.name, a.runtime from agents a
         join profile_settings ps on ps.agent_id=a.id
         join project_registrations pr on pr.agent_id=a.id and pr.team_id=a.team_id
         where a.team_id=?1 and pr.project_path=?2 and a.name=?3",
        params![team_id, project, name],
        |row| {
            Ok(AgentRef {
                team_id: team_id.clone(),
                agent_id: row.get(0)?,
                runtime: row.get(2)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow!("unknown_profile: {name}"))
}

fn current_session_key(
    conn: &Connection,
    project: &str,
    explicit: Option<String>,
) -> Result<String> {
    if let Some(value) = explicit.filter(|value| !value.trim().is_empty()) {
        return Ok(value);
    }
    if let Some(value) = stable_session_id() {
        return Ok(value);
    }
    let fallback: Option<String> = conn
        .query_row(
            "select session_key from sessions where project_path=?1 and source='fallback' order by created_at asc limit 1",
            params![project],
            |row| row.get(0),
        )
        .optional()?;
    Ok(fallback.unwrap_or_else(|| format!("fallback-{}", id())))
}

fn current_session(
    conn: &Connection,
    project: &str,
    runtime: Option<&str>,
    explicit_key: Option<&str>,
) -> Result<Identity> {
    let runtime = runtime.unwrap_or_else(|| detect_runtime().as_str());
    let session_key = current_session_key(conn, project, explicit_key.map(ToOwned::to_owned))?;
    let source = if stable_session_id().as_deref() == Some(session_key.as_str()) {
        "host"
    } else if explicit_key.is_some() {
        "explicit"
    } else {
        "fallback"
    };
    let team_id = project_team_id(conn, project)?;
    if let Some(identity) = session_identity_by_key(conn, project, &session_key)? {
        conn.execute(
            "update sessions set runtime=?1, updated_at=?2, last_seen_at=?2 where project_path=?3 and session_key=?4",
            params![runtime, now(), project, session_key],
        )?;
        return Ok(identity);
    }
    let session_name = session_display_name(&session_key);
    let agent_id = get_or_create_agent(conn, &team_id, &session_name, runtime)?;
    register_project_agent(conn, &team_id, &agent_id, project, runtime)?;
    let now = now();
    conn.execute(
        "insert into sessions (id, team_id, agent_id, project_path, session_key, alias, runtime, source, created_at, updated_at, last_seen_at)
         values (?1, ?2, ?3, ?4, ?5, null, ?6, ?7, ?8, ?8, ?8)
         on conflict(project_path, session_key) do update set
           runtime=excluded.runtime,
           updated_at=excluded.updated_at,
           last_seen_at=excluded.last_seen_at",
        params![id(), team_id, agent_id, project, session_key, runtime, source, now],
    )?;
    session_identity_by_key(conn, project, &session_key)?
        .ok_or_else(|| anyhow!("session_registration_failed: {session_key}"))
}

fn session_identity_by_key(
    conn: &Connection,
    project: &str,
    session_key: &str,
) -> Result<Option<Identity>> {
    conn.query_row(
        "select s.team_id, t.name, s.agent_id, coalesce(s.alias, a.name), s.runtime
         from sessions s
         join teams t on t.id=s.team_id
         join agents a on a.id=s.agent_id
         where s.project_path=?1 and s.session_key=?2",
        params![project, session_key],
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

fn sender_session(conn: &Connection, project: &str, explicit: Option<&str>) -> Result<Identity> {
    if let Some(name) = explicit.filter(|value| !value.trim().is_empty()) {
        return session_identity_by_name(conn, project, name);
    }
    current_session(conn, project, None, None)
}

fn session_identity_by_name(conn: &Connection, project: &str, name: &str) -> Result<Identity> {
    let (name, _) = normalize_session_lookup(name);
    conn.query_row(
        "select s.team_id, t.name, s.agent_id, coalesce(s.alias, a.name), s.runtime
         from sessions s
         join teams t on t.id=s.team_id
         join agents a on a.id=s.agent_id
         where s.project_path=?1 and (s.session_key=?2 or s.alias=?2 or a.name=?2)",
        params![project, name],
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
    .optional()?
    .ok_or_else(|| anyhow!("unknown_session: {name}"))
}

fn session_by_name(conn: &Connection, project: &str, name: &str) -> Result<AgentRef> {
    let (name, _) = normalize_session_lookup(name);
    conn.query_row(
        "select s.team_id, s.agent_id, coalesce(s.alias, a.name), s.runtime
         from sessions s
         join agents a on a.id=s.agent_id
         where s.project_path=?1 and (s.session_key=?2 or s.alias=?2 or a.name=?2)",
        params![project, name],
        |row| {
            Ok(AgentRef {
                team_id: row.get(0)?,
                agent_id: row.get(1)?,
                runtime: row.get(3)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow!("unknown_session: {name}"))
}

fn recipient_session_for_send(conn: &Connection, project: &str, name: &str) -> Result<AgentRef> {
    let (lookup, explicit_session) = normalize_session_lookup(name);
    let recipient = session_by_name(conn, project, lookup)?;
    if !explicit_session && profile_exists(conn, project, lookup)? {
        bail!(
            "ambiguous_destination: '{lookup}' matches both a session alias and a profile. Use `handoff to @{lookup}` for the session inbox or `handoff delegate {lookup}` for the profile."
        );
    }
    Ok(recipient)
}

fn list_sessions(conn: &Connection, project: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "select s.agent_id, s.session_key, s.alias, s.runtime, s.source, s.last_seen_at
         from sessions s
         where s.project_path=?1
         order by s.last_seen_at desc",
    )?;
    let rows = stmt.query_map(params![project], |row| {
        Ok(json!({
            "agent_id": row.get::<_, String>(0)?,
            "session_key": row.get::<_, String>(1)?,
            "alias": row.get::<_, Option<String>>(2)?,
            "runtime": row.get::<_, String>(3)?,
            "source": row.get::<_, String>(4)?,
            "last_seen_at": row.get::<_, String>(5)?,
        }))
    })?;
    let sessions = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(sessions
        .into_iter()
        .map(with_session_address_and_reachability)
        .collect())
}

fn session_snapshot_by_name(conn: &Connection, project: &str, name: &str) -> Result<Option<Value>> {
    let (name, _) = normalize_session_lookup(name);
    let row = conn
        .query_row(
            "select s.agent_id, s.session_key, s.alias, s.runtime, s.source, s.last_seen_at
             from sessions s
             join agents a on a.id=s.agent_id
             where s.project_path=?1 and (s.session_key=?2 or s.alias=?2 or a.name=?2)",
            params![project, name],
            |row| {
                Ok(json!({
                    "agent_id": row.get::<_, String>(0)?,
                    "session_key": row.get::<_, String>(1)?,
                    "alias": row.get::<_, Option<String>>(2)?,
                    "runtime": row.get::<_, String>(3)?,
                    "source": row.get::<_, String>(4)?,
                    "last_seen_at": row.get::<_, String>(5)?,
                }))
            },
        )
        .optional()?;
    Ok(row.map(with_session_address_and_reachability))
}

fn with_session_address_and_reachability(mut session: Value) -> Value {
    let address = session["alias"]
        .as_str()
        .map(|alias| format!("@{alias}"))
        .unwrap_or_else(|| {
            session["session_key"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        });
    let reachability = session_reachability(session["last_seen_at"].as_str().unwrap_or_default());
    session["address"] = json!(address);
    session["reachability"] = json!(reachability);
    session
}

fn session_reachability(last_seen_at: &str) -> &'static str {
    let Ok(last_seen) = DateTime::parse_from_rfc3339(last_seen_at) else {
        return "unknown";
    };
    let age = Utc::now().signed_duration_since(last_seen.with_timezone(&Utc));
    if age.num_minutes() <= 10 {
        "active"
    } else {
        "stale"
    }
}

fn route_candidates(conn: &Connection, project: &str, capability: &str) -> Result<Vec<Value>> {
    let profiles = list_profiles(conn, project)?;
    let mut candidates = Vec::new();
    for profile in profiles {
        let options = profile["options"].clone();
        if !profile_has_capability(&options, capability) {
            continue;
        }
        let profile_name = profile["name"].as_str().unwrap_or_default();
        let session_ref = options["session"]
            .as_str()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("@{profile_name}"));
        if let Some(session) = session_snapshot_by_name(conn, project, &session_ref)? {
            candidates.push(json!({
                "profile": profile_name,
                "runtime": profile["runtime"],
                "capability": options["capability"],
                "session": session["address"],
                "session_key": session["session_key"],
                "reachability": session["reachability"],
                "last_seen_at": session["last_seen_at"],
            }));
        }
    }
    candidates.sort_by(|a, b| {
        let ar = a["reachability"].as_str().unwrap_or_default();
        let br = b["reachability"].as_str().unwrap_or_default();
        (br == "active").cmp(&(ar == "active"))
    });
    Ok(candidates)
}

fn profile_has_capability(options: &Value, requested: &str) -> bool {
    let requested = requested.trim();
    options["capability"].as_str().is_some_and(|capabilities| {
        capabilities
            .split([',', ' '])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .any(|value| value == requested)
    })
}

fn set_session_alias(conn: &Connection, project: &str, agent_id: &str, alias: &str) -> Result<()> {
    let alias = normalize_session_alias(alias)?;
    let existing: Option<String> = conn
        .query_row(
            "select agent_id from sessions where project_path=?1 and alias=?2",
            params![project, alias],
            |row| row.get(0),
        )
        .optional()?;
    if existing
        .as_deref()
        .is_some_and(|existing| existing != agent_id)
    {
        bail!("session_alias_taken: {alias}");
    }
    conn.execute(
        "update sessions set alias=?1, updated_at=?2 where project_path=?3 and agent_id=?4",
        params![alias, now(), project, agent_id],
    )?;
    conn.execute(
        "update agents set name=?1, updated_at=?2 where id=?3",
        params![alias, now(), agent_id],
    )?;
    Ok(())
}

fn normalize_session_alias(alias: &str) -> Result<String> {
    let alias = alias.trim().trim_start_matches('@').trim();
    if alias.is_empty() {
        bail!("invalid_arguments: alias cannot be empty");
    }
    Ok(alias.to_string())
}

fn normalize_session_lookup(name: &str) -> (&str, bool) {
    let name = name.trim();
    if let Some(alias) = name.strip_prefix('@') {
        (alias.trim(), true)
    } else {
        (name, false)
    }
}

fn profile_exists(conn: &Connection, project: &str, name: &str) -> Result<bool> {
    let team_id = project_team_id(conn, project)?;
    let exists = conn
        .query_row(
            "select 1 from agents a
             join profile_settings ps on ps.agent_id=a.id
             join project_registrations pr on pr.agent_id=a.id and pr.team_id=a.team_id
             where a.team_id=?1 and pr.project_path=?2 and a.name=?3
             limit 1",
            params![team_id, project, name],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(exists)
}

fn session_display_name(session_key: &str) -> String {
    let hash = content_hash(session_key);
    format!("session-{}", &hash[..8])
}

fn agent_by_name(conn: &Connection, team_id: &str, name: &str) -> Result<AgentRef> {
    conn.query_row(
        "select id, name, runtime from agents where team_id=?1 and name=?2",
        params![team_id, name],
        |row| {
            Ok(AgentRef {
                team_id: team_id.to_string(),
                agent_id: row.get(0)?,
                runtime: row.get(2)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow!("unknown_agent: {name}"))
}

#[derive(Debug)]
struct NewMessage<'a> {
    sender: &'a Identity,
    recipient_agent_id: &'a str,
    thread_id: Option<&'a str>,
    kind: &'a str,
    context_id: Option<&'a str>,
    job_id: Option<&'a str>,
    subject: Option<&'a str>,
    body: &'a str,
}

fn create_message(conn: &Connection, message: NewMessage<'_>) -> Result<String> {
    let tx = conn.unchecked_transaction()?;
    let message_id = create_message_in_tx(&tx, message)?;
    tx.commit()?;
    Ok(message_id)
}

fn create_message_in_tx(conn: &Connection, message: NewMessage<'_>) -> Result<String> {
    let sender = message.sender;
    let thread_id = if let Some(thread_id) = message.thread_id {
        thread_id.to_string()
    } else {
        let thread_id = id();
        let now = now();
        conn.execute(
            "insert into threads (id, team_id, subject, created_by_agent_id, created_at, updated_at) values (?1, ?2, ?3, ?4, ?5, ?5)",
            params![thread_id, sender.team_id, message.subject, sender.agent_id, now],
        )?;
        append_event(
            conn,
            "thread.created",
            Some(&sender.team_id),
            Some(&sender.agent_id),
            Some(&thread_id),
            json!({"subject": message.subject}),
        )?;
        thread_id
    };
    let message_id = id();
    let now = now();
    conn.execute(
        "insert into messages (id, team_id, thread_id, from_agent_id, to_agent_id, kind, context_id, job_id, subject, body, created_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            message_id,
            sender.team_id,
            thread_id,
            sender.agent_id,
            message.recipient_agent_id,
            message.kind,
            message.context_id,
            message.job_id,
            message.subject,
            message.body,
            now
        ],
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
        json!({"kind": message.kind, "thread_id": thread_id}),
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

fn mark_message_read(conn: &Connection, identity: &Identity, message_id: &str) -> Result<()> {
    let inserted = conn.execute(
        "insert or ignore into message_reads (message_id, agent_id, read_at) values (?1, ?2, ?3)",
        params![message_id, identity.agent_id, now()],
    )?;
    if inserted > 0 {
        append_event(
            conn,
            "message.read",
            Some(&identity.team_id),
            Some(&identity.agent_id),
            Some(message_id),
            json!({"message_id": message_id}),
        )?;
    }
    Ok(())
}

fn deliver_monitor_messages(
    conn: &Connection,
    identities: &[Identity],
    json_output: bool,
) -> Result<usize> {
    let mut delivered = 0;
    for identity in identities {
        let messages = inbox_messages(conn, &identity.agent_id, true, DEFAULT_LIMIT)?;
        for message in messages {
            let message_id = message["id"].as_str().unwrap_or_default();
            if json_output {
                println!("{}", serde_json::to_string(&message)?);
            } else {
                println!(
                    "{} | {} | {} -> {} | {}",
                    message["created_at"].as_str().unwrap_or_default(),
                    message["team"].as_str().unwrap_or_default(),
                    message["from"].as_str().unwrap_or_default(),
                    message["to"].as_str().unwrap_or_default(),
                    escape_monitor_body(message["body"].as_str().unwrap_or_default())
                );
            }
            io::stdout().flush()?;
            mark_message_read(conn, identity, message_id)?;
            delivered += 1;
        }
    }
    Ok(delivered)
}

fn write_notify_files(conn: &Connection, project: &str) -> Result<usize> {
    let sessions = sessions_for_notify(conn, project)?;
    let mut written = 0;
    for session in sessions {
        let messages = inbox_messages(conn, &session.agent_id, true, DEFAULT_LIMIT)?;
        let path = notify_path(project, &session.session_key)?;
        if messages.is_empty() {
            if path.exists() {
                let _ = fs::remove_file(path);
            }
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, notify_markdown(&session, &messages))?;
        written += 1;
    }
    Ok(written)
}

#[derive(Debug)]
struct NotifySession {
    agent_id: String,
    session_key: String,
    label: String,
}

fn sessions_for_notify(conn: &Connection, project: &str) -> Result<Vec<NotifySession>> {
    let mut stmt = conn.prepare(
        "select s.agent_id, s.session_key, coalesce(s.alias, a.name)
         from sessions s
         join agents a on a.id=s.agent_id
         where s.project_path=?1
         order by s.updated_at desc",
    )?;
    let rows = stmt.query_map(params![project], |row| {
        Ok(NotifySession {
            agent_id: row.get(0)?,
            session_key: row.get(1)?,
            label: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn notify_path(project: &str, session_key: &str) -> Result<PathBuf> {
    let dir = Path::new(project).join(".handoff").join("notify");
    Ok(dir.join(format!("{}.md", session_display_name(session_key))))
}

fn notify_markdown(session: &NotifySession, messages: &[Value]) -> String {
    let mut output = format!(
        "# handoff notifications for {}\n\nProcess these incoming handoff messages now. If a reply is needed, use `handoff reply <thread-id> <message>`.\n\n",
        session.label
    );
    for message in messages {
        let id = message["id"].as_str().unwrap_or_default();
        output.push_str(&format!("<!-- handoff-message-id: {id} -->\n"));
        output.push_str(&format!(
            "## {} -> {}\n\n{}\n\n",
            message["from"].as_str().unwrap_or_default(),
            message["to"].as_str().unwrap_or_default(),
            message["body"].as_str().unwrap_or_default()
        ));
        if let Some(context_id) = message["context_id"].as_str() {
            output.push_str(&format!("Context: `{context_id}`\n\n"));
        }
        if let Some(thread_id) = message["thread_id"].as_str() {
            output.push_str(&format!("Thread: `{thread_id}`\n\n"));
        }
    }
    output
}

fn hook_response_json(body: &str) -> Value {
    if body.trim().is_empty() {
        return json!({
            "continue": true,
            "systemMessage": "handoff: no new messages"
        });
    }
    json!({
        "decision": "block",
        "reason": body
    })
}

fn notify_message_ids(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("<!-- handoff-message-id: ")
                .and_then(|rest| rest.strip_suffix(" -->"))
                .map(ToOwned::to_owned)
        })
        .collect()
}

fn escape_monitor_body(body: &str) -> String {
    body.replace('\r', "").replace('\n', "\\n")
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

#[derive(Debug)]
struct ContextInputSpec<'a> {
    text: Option<&'a str>,
    stdin: bool,
    file: Option<&'a Path>,
    files: &'a [PathBuf],
    git_diff: bool,
    cmd: Option<&'a str>,
}

impl ContextInputSpec<'_> {
    fn has_inputs(&self) -> bool {
        self.text.is_some()
            || self.stdin
            || self.file.is_some()
            || !self.files.is_empty()
            || self.git_diff
            || self.cmd.is_some()
    }
}

fn add_context_inputs(
    conn: &Connection,
    context_id: &str,
    inputs: ContextInputSpec<'_>,
) -> Result<()> {
    let items = capture_context_inputs(inputs)?;
    persist_context_items(conn, context_id, &items)
}

#[derive(Debug)]
struct CapturedContextItem {
    kind: &'static str,
    label: Option<String>,
    source: Option<String>,
    content: String,
    metadata: Value,
}

fn capture_context_inputs(inputs: ContextInputSpec<'_>) -> Result<Vec<CapturedContextItem>> {
    let mut items = Vec::new();
    if let Some(text) = inputs.text {
        items.push(CapturedContextItem {
            kind: "text",
            label: Some("text".to_string()),
            source: None,
            content: text.to_string(),
            metadata: json!({}),
        });
    }
    if inputs.stdin {
        let content = read_all_stdin()?;
        items.push(CapturedContextItem {
            kind: "stdin",
            label: Some("stdin".to_string()),
            source: None,
            content,
            metadata: json!({}),
        });
    }
    if let Some(file) = inputs.file {
        items.push(capture_file_context(file)?);
    }
    for file in inputs.files {
        items.push(capture_file_context(file)?);
    }
    if inputs.git_diff {
        let output = Command::new("git")
            .args(["diff", "--no-ext-diff", "HEAD"])
            .output()
            .or_else(|_| Command::new("git").args(["diff", "--no-ext-diff"]).output())
            .context("capture git diff")?;
        let content = String::from_utf8_lossy(&output.stdout).to_string();
        items.push(CapturedContextItem {
            kind: "git_diff",
            label: Some("git diff".to_string()),
            source: Some("git diff --no-ext-diff HEAD".to_string()),
            content,
            metadata: json!({"status": output.status.code()}),
        });
    }
    if let Some(cmd) = inputs.cmd {
        let output = shell_command(cmd)
            .output()
            .context("capture command output")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let content = format!("$ {cmd}\n\n[stdout]\n{stdout}\n[stderr]\n{stderr}");
        items.push(CapturedContextItem {
            kind: "command_output",
            label: Some(cmd.to_string()),
            source: Some(cmd.to_string()),
            content,
            metadata: json!({"status": output.status.code()}),
        });
    }
    if items.is_empty() {
        bail!(
            "context_capture_failed: provide --text, --stdin, --file, --files, --git-diff, or --cmd"
        );
    }
    Ok(items)
}

fn persist_context_items(
    conn: &Connection,
    context_id: &str,
    items: &[CapturedContextItem],
) -> Result<()> {
    for item in items {
        add_context_item(
            conn,
            context_id,
            item.kind,
            item.label.as_deref(),
            item.source.as_deref(),
            &item.content,
            item.metadata.clone(),
        )?;
    }
    Ok(())
}

fn capture_file_context(file: &Path) -> Result<CapturedContextItem> {
    let content =
        fs::read_to_string(file).with_context(|| format!("read file {}", file.display()))?;
    let metadata = fs::metadata(file)?;
    Ok(CapturedContextItem {
        kind: "file",
        label: file
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned),
        source: Some(file.display().to_string()),
        content,
        metadata: json!({"path": file.display().to_string(), "size": metadata.len()}),
    })
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
        "select j.team_id, t.name, j.thread_id, j.task_message_id, j.context_id, j.requested_by_agent_id, j.target_agent_id, a.name, j.runtime, j.timeout_seconds
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
                timeout_seconds: row.get(9)?,
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

fn job_logs_after(
    conn: &Connection,
    job_id: &str,
    after_created_at: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "select stream, line, created_at from job_logs where job_id=?1 and (?2 is null or created_at > ?2) order by created_at asc",
    )?;
    let rows = stmt.query_map(params![job_id, after_created_at], |row| {
        Ok(json!({"stream": row.get::<_, String>(0)?, "line": row.get::<_, String>(1)?, "created_at": row.get::<_, String>(2)?}))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn print_log_line(log: &serde_json::Value) {
    println!(
        "{} {}",
        log["stream"].as_str().unwrap_or_default(),
        log["line"].as_str().unwrap_or_default()
    );
}

fn follow_job_logs(conn: &Connection, job_id: &str, last_created_at: Option<&str>) -> Result<()> {
    let mut last_created_at = last_created_at.map(ToOwned::to_owned);
    loop {
        let logs = job_logs_after(conn, job_id, last_created_at.as_deref())?;
        for log in &logs {
            print_log_line(log);
            last_created_at = log["created_at"].as_str().map(ToOwned::to_owned);
        }
        io::stdout().flush()?;
        if is_terminal_job_state(&job_state(conn, job_id)?) {
            let logs = job_logs_after(conn, job_id, last_created_at.as_deref())?;
            for log in &logs {
                print_log_line(log);
            }
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn print_job_status(job: &serde_json::Value) {
    let id = job["id"].as_str().unwrap_or_default();
    let state = job["state"].as_str().unwrap_or_default();
    println!("job {id}");
    println!("state: {state}");
    println!(
        "route: {} -> {} ({})",
        job["requested_by"].as_str().unwrap_or_default(),
        job["target"].as_str().unwrap_or_default(),
        job["runtime"].as_str().unwrap_or_default()
    );
    if let Some(context_id) = job["context_id"].as_str() {
        println!("context: {context_id}");
    }
    if let Some(result_message_id) = job["result_message_id"].as_str() {
        println!("result_message: {result_message_id}");
        println!("next: handoff result {id}");
    }
    if let Some(code) = job["failure_code"].as_str() {
        println!("failure: {code}");
    }
    if let Some(message) = job["failure_message"].as_str()
        && !message.trim().is_empty()
    {
        println!("detail: {}", first_line(message));
    }
    match state {
        "queued" | "starting" | "running" => {
            println!("next: handoff logs {id}");
        }
        "blocked" => {
            let target = job["target"].as_str().unwrap_or("<agent>");
            let runtime = job["runtime"].as_str().unwrap_or("<runtime>");
            println!(
                "next: configure HANDOFF_AGENT_CMD_{} or HANDOFF_RUNTIME_CMD_{}, then run handoff retry {id}",
                env_key(target),
                env_key(runtime)
            );
        }
        "failed" | "timeout" => {
            println!("next: handoff logs {id}");
            println!("retry: handoff retry {id}");
        }
        _ => {}
    }
}

fn is_terminal_job_state(state: &str) -> bool {
    matches!(
        state,
        "succeeded" | "failed" | "cancelled" | "timeout" | "blocked"
    )
}

fn job_state(conn: &Connection, job_id: &str) -> Result<String> {
    conn.query_row(
        "select state from jobs where id=?1",
        params![job_id],
        |row| row.get(0),
    )
    .with_context(|| format!("read job state for {job_id}"))
}

fn set_job_state(
    conn: &Connection,
    job_id: &str,
    state: &str,
    failure_code: Option<&str>,
    failure_message: Option<&str>,
) -> Result<bool> {
    let updated = match state {
        "starting" | "running" => {
            let sql = format!(
                "update jobs set state=?1, started_at=coalesce(started_at, ?2), failure_code=?3, failure_message=?4 where id=?5 and state not in ({TERMINAL_JOB_STATE_SQL})"
            );
            conn.execute(
                &sql,
                params![state, now(), failure_code, failure_message, job_id],
            )?
        }
        "blocked" | "failed" | "cancelled" | "timeout" | "succeeded" => {
            let sql = format!(
                "update jobs set state=?1, finished_at=coalesce(finished_at, ?2), failure_code=?3, failure_message=?4, process_id=null where id=?5 and state not in ({TERMINAL_JOB_STATE_SQL})"
            );
            conn.execute(
                &sql,
                params![state, now(), failure_code, failure_message, job_id],
            )?
        }
        _ => bail!("invalid job state: {state}"),
    };
    if updated > 0 {
        append_event(
            conn,
            &format!("job.{state}"),
            None,
            None,
            Some(job_id),
            json!({"job_id": job_id, "state": state}),
        )?;
    }
    Ok(updated > 0)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdapterKind {
    Shell,
    BuiltInJson,
    CodexJson,
}

#[derive(Debug)]
struct AdapterCommand {
    command: String,
    kind: AdapterKind,
}

fn adapter_command(runtime: &str, agent_name: &str, task: &str) -> Option<AdapterCommand> {
    let agent_key = format!("HANDOFF_AGENT_CMD_{}", env_key(agent_name));
    if let Ok(command) = env::var(agent_key) {
        return Some(AdapterCommand {
            command,
            kind: AdapterKind::Shell,
        });
    }
    let runtime_key = format!("HANDOFF_RUNTIME_CMD_{}", env_key(runtime));
    if let Ok(command) = env::var(runtime_key) {
        return Some(AdapterCommand {
            command,
            kind: AdapterKind::Shell,
        });
    }
    match runtime {
        "shell" => Some(AdapterCommand {
            command: task.to_string(),
            kind: AdapterKind::Shell,
        }),
        "claude-code" => Some(AdapterCommand {
            command: r#"claude -p "$HANDOFF_PROMPT" --output-format json"#.to_string(),
            kind: AdapterKind::BuiltInJson,
        }),
        "codex" => Some(AdapterCommand {
            command: r#"codex exec "$HANDOFF_PROMPT" --json"#.to_string(),
            kind: AdapterKind::CodexJson,
        }),
        _ => None,
    }
}

fn agent_prompt(system_prompt: Option<&str>, task: &str, context: &str) -> String {
    let mut sections = Vec::new();
    if let Some(system_prompt) = system_prompt.filter(|value| !value.trim().is_empty()) {
        sections.push(system_prompt.to_string());
    }
    sections.push(task.to_string());
    if !context.trim().is_empty() {
        sections.push(format!(
            "Context follows. Use it as the authoritative input for this task.\n{context}"
        ));
    }
    sections.join("\n\n")
}

fn adapter_result_body(adapter: &AdapterCommand, stdout: &str) -> String {
    if matches!(
        adapter.kind,
        AdapterKind::BuiltInJson | AdapterKind::CodexJson
    ) {
        extract_json_result_text(stdout).unwrap_or_else(|| stdout.to_string())
    } else {
        stdout.to_string()
    }
}

fn extract_json_result_text(stdout: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(stdout)
        && let Some(text) = extract_json_text_field(&value)
    {
        return Some(text);
    }
    let parsed_lines = stdout
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect::<Vec<_>>();
    parsed_lines
        .iter()
        .find_map(extract_agent_message_text)
        .or_else(|| {
            parsed_lines
                .iter()
                .find(|value| !looks_like_codex_event(value))
                .and_then(extract_json_text_field)
        })
}

fn extract_agent_message_text(value: &Value) -> Option<String> {
    let item = value.get("item").unwrap_or(value);
    if item.get("type").and_then(Value::as_str) == Some("agent_message") {
        return extract_json_text_field(item);
    }
    None
}

fn looks_like_codex_event(value: &Value) -> bool {
    value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|event_type| event_type.contains('.'))
        || value.get("item").is_some()
}

fn extract_json_text_field(value: &Value) -> Option<String> {
    const TEXT_FIELDS: &[&str] = &[
        "result",
        "text",
        "message",
        "output",
        "answer",
        "final_answer",
        "content",
    ];
    match value {
        Value::Object(map) => {
            let mut preferred = Vec::new();
            for field in TEXT_FIELDS {
                if let Some(value) = map.get(*field) {
                    if let Some(text) = value.as_str().filter(|text| !text.trim().is_empty()) {
                        preferred.push(text.to_string());
                    } else if let Some(text) = extract_json_text_field(value) {
                        preferred.push(text);
                    }
                }
            }
            if !preferred.is_empty() {
                return Some(preferred.join("\n"));
            }
            collect_json_text_fragments(map.values()).map(|items| items.join("\n"))
        }
        Value::Array(items) => {
            collect_json_text_fragments(items.iter()).map(|items| items.join("\n"))
        }
        _ => None,
    }
}

fn collect_json_text_fragments<'a>(
    values: impl IntoIterator<Item = &'a Value>,
) -> Option<Vec<String>> {
    let items = values
        .into_iter()
        .filter_map(extract_json_text_field)
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>();
    if items.is_empty() { None } else { Some(items) }
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

#[derive(Debug)]
struct JobOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

fn wait_for_child_with_logs(
    conn: &Connection,
    job_id: &str,
    child: &mut Child,
    timeout_seconds: Option<i64>,
) -> Result<JobOutput> {
    let (tx, rx) = mpsc::channel();
    let stdout_reader = spawn_pipe_reader("stdout", child.stdout.take(), tx.clone());
    let stderr_reader = spawn_pipe_reader("stderr", child.stderr.take(), tx);
    let deadline = timeout_seconds
        .filter(|value| *value > 0)
        .map(|value| Instant::now() + Duration::from_secs(value as u64));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let (status, timed_out) = loop {
        drain_log_messages(conn, job_id, &rx, &mut stdout, &mut stderr)?;
        if let Some(status) = child.try_wait().context("poll adapter command")? {
            break (status, false);
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            let _ = child.kill();
            let status = child.wait().context("wait for timed out adapter command")?;
            break (status, true);
        }
        thread::sleep(Duration::from_millis(50));
    };

    join_pipe_reader(stdout_reader)?;
    join_pipe_reader(stderr_reader)?;
    drain_log_messages(conn, job_id, &rx, &mut stdout, &mut stderr)?;

    Ok(JobOutput {
        status,
        stdout: stdout.join("\n"),
        stderr: stderr.join("\n"),
        timed_out,
    })
}

fn spawn_pipe_reader<R: Read + Send + 'static>(
    stream: &'static str,
    pipe: Option<R>,
    tx: Sender<(String, String)>,
) -> Option<thread::JoinHandle<io::Result<()>>> {
    pipe.map(|pipe| {
        thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut line = String::new();
            loop {
                line.clear();
                let bytes = reader.read_line(&mut line)?;
                if bytes == 0 {
                    break;
                }
                let line = line.trim_end_matches(['\r', '\n']).to_string();
                if tx.send((stream.to_string(), line)).is_err() {
                    break;
                }
            }
            Ok(())
        })
    })
}

fn join_pipe_reader(handle: Option<thread::JoinHandle<io::Result<()>>>) -> Result<()> {
    if let Some(handle) = handle {
        handle
            .join()
            .map_err(|_| anyhow!("adapter log reader panicked"))??;
    }
    Ok(())
}

fn drain_log_messages(
    conn: &Connection,
    job_id: &str,
    rx: &Receiver<(String, String)>,
    stdout: &mut Vec<String>,
    stderr: &mut Vec<String>,
) -> Result<()> {
    while let Ok((stream, line)) = rx.try_recv() {
        append_log(conn, job_id, &stream, &line)?;
        match stream.as_str() {
            "stdout" => stdout.push(line),
            "stderr" => stderr.push(line),
            _ => {}
        }
    }
    Ok(())
}

fn create_send_context_if_requested(
    conn: &Connection,
    sender: &Identity,
    args: &SendArgs,
) -> Result<Option<String>> {
    let has_context_input = args.git_diff || args.as_context || args.cmd.is_some();
    if args.context.is_some() && has_context_input {
        bail!("invalid_arguments: use either --context or context capture flags, not both");
    }
    if args.as_context && args.file.is_none() {
        bail!("invalid_arguments: --as-context requires --file");
    }
    if !has_context_input {
        return Ok(None);
    }
    let context_id = create_context(conn, sender, args.subject.as_deref())?;
    add_context_inputs(
        conn,
        &context_id,
        ContextInputSpec {
            text: None,
            stdin: false,
            file: if args.as_context {
                args.file.as_deref()
            } else {
                None
            },
            files: &[],
            git_diff: args.git_diff,
            cmd: args.cmd.as_deref(),
        },
    )?;
    append_event(
        conn,
        "context.created",
        Some(&sender.team_id),
        Some(&sender.agent_id),
        Some(&context_id),
        json!({"context_id": context_id, "source": "send"}),
    )?;
    Ok(Some(context_id))
}

fn read_send_body(args: &SendArgs, has_context: bool) -> Result<String> {
    if has_context {
        let file_body = if args.as_context {
            None
        } else {
            args.file.as_deref()
        };
        let mut sources = 0;
        if !args.message.is_empty() {
            sources += 1;
        }
        if args.stdin {
            sources += 1;
        }
        if file_body.is_some() {
            sources += 1;
        }
        if args.message_text.is_some() {
            sources += 1;
        }
        if sources == 0 {
            return Ok("Context handoff".to_string());
        }
        return read_message_body(
            &args.message,
            args.stdin,
            file_body,
            args.message_text.as_deref(),
        );
    }
    read_message_body(
        &args.message,
        args.stdin,
        args.file.as_deref(),
        args.message_text.as_deref(),
    )
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

fn session_id_from_hook_stdin() -> Result<Option<String>> {
    let input = read_all_stdin()?;
    if input.trim().is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    Ok(value
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned))
}

fn stable_session_id() -> Option<String> {
    [
        "HANDOFF_SESSION_ID",
        "CLAUDE_CODE_SESSION_ID",
        "CODEX_SESSION_ID",
    ]
    .iter()
    .find_map(|key| env::var(key).ok().filter(|value| !value.trim().is_empty()))
}

fn claim_monitor_pid(session_id: &str) -> Result<()> {
    cleanup_stale_monitor_pidfiles()?;
    let path = monitor_pid_path(session_id)?;
    if let Ok(content) = fs::read_to_string(&path)
        && let Ok(pid) = content.trim().parse::<u32>()
        && pid != std::process::id()
        && pid_alive(pid)
    {
        terminate_pid(pid)?;
    }
    fs::write(path, std::process::id().to_string())?;
    Ok(())
}

fn cleanup_stale_monitor_pidfiles() -> Result<()> {
    let run = run_dir()?;
    if !run.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(run)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("monitor.") || !name.ends_with(".pid") {
            continue;
        }
        let pid = fs::read_to_string(&path)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok());
        if pid.is_none_or(|pid| !pid_alive(pid)) {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

fn monitor_pid_path(session_id: &str) -> Result<PathBuf> {
    let key = content_hash(session_id);
    Ok(run_dir()?.join(format!("monitor.{key}.pid")))
}

fn run_dir() -> Result<PathBuf> {
    let path = app_home()?.join("run");
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn terminate_pid(pid: u32) -> Result<()> {
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
    Ok(())
}

pub(crate) fn shell_quote(value: &str) -> String {
    value.replace('\'', "'\\''")
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

fn error_hint(message: &str) -> Option<&'static str> {
    if message.contains("multiple_identities") {
        Some("  handoff sessions\n  handoff session alias <alias>")
    } else if message.contains("not_joined") {
        Some("  handoff session alias <alias>\n  handoff profile create <profile> --runtime shell")
    } else if message.contains("unknown_agent") {
        Some("  handoff sessions\n  handoff profile list")
    } else if message.contains("unknown_profile") {
        Some("  handoff profile list\n  handoff profile create <profile> --runtime shell")
    } else if message.contains("unknown_session") {
        Some(
            "  handoff sessions\n  handoff session alias <alias>\n  handoff to @<alias> \"message\"",
        )
    } else if message.contains("ambiguous_destination") {
        Some("  handoff to @<alias> \"message\"\n  handoff delegate <profile> --task \"...\"")
    } else if message.contains("context_capture_failed") {
        Some(
            "  handoff context create --text \"notes\"\n  handoff context create --git-diff\n  handoff context create --file <path>",
        )
    } else if message.contains("provide exactly one message source") {
        Some(
            "  handoff to @<alias> \"message\"\n  command | handoff to @<alias> --stdin\n  handoff to @<alias> --file <path>",
        )
    } else if message.contains("job_not_finished") {
        Some("  handoff status <job-id>\n  handoff logs <job-id>")
    } else if message.contains("role_locked") {
        Some("  Use a different session alias or wait for the legacy role lease to expire.")
    } else {
        None
    }
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or("")
}

fn identity_json(identity: &Identity) -> serde_json::Value {
    json!({"team": identity.team, "agent": identity.agent, "runtime": identity.runtime})
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
    use clap::CommandFactory;
    use tempfile::TempDir;

    fn test_conn() -> (TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("handoff.db")).unwrap();
        ensure_schema(&conn).unwrap();
        (dir, conn)
    }

    fn test_project() -> String {
        project_path(None).unwrap()
    }

    fn current_test_session(conn: &Connection, alias: &str) -> Identity {
        let project = test_project();
        let session = current_session(conn, &project, Some("shell"), None).unwrap();
        set_session_alias(conn, &project, &session.agent_id, alias).unwrap();
        current_session(conn, &project, Some("shell"), None).unwrap()
    }

    fn named_test_session(conn: &Connection, key: &str, alias: &str) -> Identity {
        let project = test_project();
        let session = current_session(conn, &project, Some("shell"), Some(key)).unwrap();
        set_session_alias(conn, &project, &session.agent_id, alias).unwrap();
        current_session(conn, &project, Some("shell"), Some(key)).unwrap()
    }

    fn test_profile(conn: &Connection, name: &str) {
        cmd_profile_create(
            conn,
            ProfileCreateArgs {
                name: name.into(),
                runtime: Some(Runtime::Shell),
                prompt: None,
                prompt_file: None,
                project: None,
                json: false,
            },
        )
        .unwrap();
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn join_send_and_inbox_work() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "alice");
        let bob = named_test_session(&conn, "bob-session", "bob");
        cmd_send(
            &conn,
            SendArgs {
                session: "@bob".into(),
                project: None,
                message: vec!["hello".into()],
                as_agent: None,
                team: None,
                stdin: false,
                file: None,
                as_context: false,
                git_diff: false,
                cmd: None,
                subject: None,
                thread: None,
                context: None,
                message_text: None,
                json: false,
            },
            "message",
        )
        .unwrap();
        let inbox = inbox_messages(&conn, &bob.agent_id, true, 10).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0]["body"], "hello");
    }

    #[test]
    fn send_to_at_alias_works_when_profile_has_same_name() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "alice");
        let reviewer = named_test_session(&conn, "reviewer-session", "reviewer");
        test_profile(&conn, "reviewer");
        cmd_send(
            &conn,
            SendArgs {
                session: "@reviewer".into(),
                project: None,
                message: vec!["hello reviewer".into()],
                as_agent: None,
                team: None,
                stdin: false,
                file: None,
                as_context: false,
                git_diff: false,
                cmd: None,
                subject: None,
                thread: None,
                context: None,
                message_text: None,
                json: false,
            },
            "message",
        )
        .unwrap();
        let inbox = inbox_messages(&conn, &reviewer.agent_id, true, 10).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0]["body"], "hello reviewer");
    }

    #[test]
    fn send_bare_name_is_ambiguous_when_session_and_profile_match() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "alice");
        named_test_session(&conn, "reviewer-session", "reviewer");
        test_profile(&conn, "reviewer");
        let err = cmd_send(
            &conn,
            SendArgs {
                session: "reviewer".into(),
                project: None,
                message: vec!["hello reviewer".into()],
                as_agent: None,
                team: None,
                stdin: false,
                file: None,
                as_context: false,
                git_diff: false,
                cmd: None,
                subject: None,
                thread: None,
                context: None,
                message_text: None,
                json: false,
            },
            "message",
        )
        .unwrap_err();
        assert!(err.to_string().contains("ambiguous_destination"));
        assert!(err.to_string().contains("handoff to @reviewer"));
    }

    #[test]
    fn route_sends_to_profile_linked_session_by_capability() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "alice");
        let reviewer = named_test_session(&conn, "reviewer-session", "reviewer");
        test_profile(&conn, "reviewer");
        cmd_profile_set(
            &conn,
            ProfileSetArgs {
                name: "reviewer".into(),
                settings: vec!["session=@reviewer".into(), "capability=review,docs".into()],
                project: None,
                json: false,
            },
        )
        .unwrap();
        cmd_route(
            &conn,
            RouteArgs {
                message: vec!["please review".into()],
                capability: "review".into(),
                project: None,
                subject: Some("Review request".into()),
                json: false,
            },
        )
        .unwrap();
        let inbox = inbox_messages(&conn, &reviewer.agent_id, true, 10).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0]["body"], "please review");
        assert_eq!(inbox[0]["subject"], "Review request");
    }

    #[test]
    fn route_reports_candidates_without_sending_when_message_is_empty() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "alice");
        named_test_session(&conn, "reviewer-session", "reviewer");
        test_profile(&conn, "reviewer");
        cmd_profile_set(
            &conn,
            ProfileSetArgs {
                name: "reviewer".into(),
                settings: vec!["session=@reviewer".into(), "capability=review".into()],
                project: None,
                json: false,
            },
        )
        .unwrap();
        let candidates = route_candidates(&conn, &test_project(), "review").unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0]["profile"], "reviewer");
        assert_eq!(candidates[0]["session"], "@reviewer");
        assert_eq!(candidates[0]["reachability"], "active");
    }

    #[test]
    fn send_honors_explicit_sender_session() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "lead");
        named_test_session(&conn, "reviewer-session", "reviewer");
        let observer = named_test_session(&conn, "observer-session", "observer");
        cmd_send(
            &conn,
            SendArgs {
                session: "observer".into(),
                project: None,
                message: vec!["from reviewer".into()],
                as_agent: Some("reviewer".into()),
                team: None,
                stdin: false,
                file: None,
                as_context: false,
                git_diff: false,
                cmd: None,
                subject: None,
                thread: None,
                context: None,
                message_text: None,
                json: false,
            },
            "message",
        )
        .unwrap();
        let inbox = inbox_messages(&conn, &observer.agent_id, true, 10).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0]["from"], "reviewer");
    }

    #[test]
    fn context_file_capture_hashes_content() {
        let (_dir, conn) = test_conn();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), "context").unwrap();
        let identity = current_test_session(&conn, "alice");
        let context_id = create_context(&conn, &identity, Some("test")).unwrap();
        let item = capture_file_context(tmp.path()).unwrap();
        persist_context_items(&conn, &context_id, &[item]).unwrap();
        let context = context_json(&conn, &context_id).unwrap();
        assert_eq!(context["items"][0]["kind"], "file");
        assert!(context["items"][0]["content_hash"].as_str().unwrap().len() > 20);
    }

    #[test]
    fn agent_prompt_appends_context() {
        let prompt = agent_prompt(None, "Review this diff.", "--- git_diff ---\n+change");
        assert!(prompt.starts_with("Review this diff."));
        assert!(prompt.contains("Context follows."));
        assert!(prompt.contains("+change"));
    }

    #[test]
    fn json_adapter_result_extracts_nested_text() {
        let stdout = r#"{"event":"done","result":{"message":{"content":[{"type":"text","text":"review complete"}]}}}"#;
        assert_eq!(
            extract_json_result_text(stdout).as_deref(),
            Some("review complete")
        );
        let jsonl = "{\"event\":\"start\"}\n{\"output\":\"final text\"}\n";
        assert_eq!(
            extract_json_result_text(jsonl).as_deref(),
            Some("final text")
        );
    }

    #[test]
    fn json_adapter_result_prefers_codex_agent_message_jsonl() {
        let jsonl = concat!(
            r#"{"type":"item.completed","item":{"type":"agent_message","content":[{"type":"text","text":"final answer"}]}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"type":"tool_result","message":"warning payload"}}"#,
            "\n",
            r#"{"type":"turn.completed"}"#,
            "\n",
        );
        assert_eq!(
            extract_json_result_text(jsonl).as_deref(),
            Some("final answer")
        );
    }

    #[test]
    fn json_adapter_result_ignores_codex_non_answer_jsonl() {
        let jsonl = concat!(
            r#"{"type":"item.completed","item":{"type":"tool_result","message":"warning payload"}}"#,
            "\n",
            r#"{"type":"turn.completed"}"#,
            "\n",
        );
        assert_eq!(extract_json_result_text(jsonl), None);
    }

    #[test]
    fn json_adapter_result_joins_multipart_content() {
        let stdout =
            r#"{"content":[{"type":"text","text":"part 1"},{"type":"text","text":"part 2"}]}"#;
        assert_eq!(
            extract_json_result_text(stdout).as_deref(),
            Some("part 1\npart 2")
        );
    }

    #[test]
    fn shell_delegate_requires_task() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "lead");
        test_profile(&conn, "reviewer");
        let err = cmd_delegate(
            &conn,
            DelegateArgs {
                profile: "reviewer".into(),
                task: None,
                context: None,
                git_diff: false,
                stdin: false,
                file: None,
                timeout: None,
                wait: false,
                as_agent: Some("lead".into()),
                subject: None,
                json: false,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("--task is required"));
    }

    #[test]
    fn send_file_as_context_creates_context_message() {
        let (_dir, conn) = test_conn();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), "handoff context").unwrap();
        current_test_session(&conn, "alice");
        let bob = named_test_session(&conn, "bob-file-session", "bob");
        cmd_send(
            &conn,
            SendArgs {
                session: "bob".into(),
                project: None,
                message: vec![],
                as_agent: Some("alice".into()),
                team: None,
                stdin: false,
                file: Some(tmp.path().to_path_buf()),
                as_context: true,
                git_diff: false,
                cmd: None,
                subject: Some("ctx".into()),
                thread: None,
                context: None,
                message_text: Some("use this".into()),
                json: false,
            },
            "message",
        )
        .unwrap();
        let inbox = inbox_messages(&conn, &bob.agent_id, true, 10).unwrap();
        assert_eq!(inbox[0]["kind"], "context");
        let context_id = inbox[0]["context_id"].as_str().unwrap();
        let context = context_json(&conn, context_id).unwrap();
        assert_eq!(context["items"][0]["content"], "handoff context");
        assert_eq!(inbox[0]["body"], "use this");
    }

    #[test]
    fn send_as_context_requires_file() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "alice");
        named_test_session(&conn, "bob-require-file-session", "bob");
        let err = cmd_send(
            &conn,
            SendArgs {
                session: "bob".into(),
                project: None,
                message: vec![],
                as_agent: Some("alice".into()),
                team: None,
                stdin: false,
                file: None,
                as_context: true,
                git_diff: false,
                cmd: None,
                subject: None,
                thread: None,
                context: None,
                message_text: None,
                json: false,
            },
            "message",
        )
        .unwrap_err();
        assert!(err.to_string().contains("--as-context requires --file"));
    }

    #[test]
    fn session_alias_rejects_conflicts() {
        let (_dir, conn) = test_conn();
        let project = test_project();
        let first = named_test_session(&conn, "session-a", "alice");
        let second = named_test_session(&conn, "session-b", "bob");
        assert_ne!(first.agent_id, second.agent_id);
        let err = set_session_alias(&conn, &project, &second.agent_id, "alice").unwrap_err();
        assert!(err.to_string().contains("session_alias_taken"));
    }

    #[test]
    fn monitor_delivery_marks_messages_read() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "alice");
        let bob = named_test_session(&conn, "bob-monitor-session", "bob");
        cmd_send(
            &conn,
            SendArgs {
                session: "bob".into(),
                project: None,
                message: vec!["stream me".into()],
                as_agent: Some("alice".into()),
                team: None,
                stdin: false,
                file: None,
                as_context: false,
                git_diff: false,
                cmd: None,
                subject: None,
                thread: None,
                context: None,
                message_text: None,
                json: false,
            },
            "message",
        )
        .unwrap();
        let identities = vec![bob.clone()];
        assert_eq!(
            deliver_monitor_messages(&conn, &identities, false).unwrap(),
            1
        );
        assert!(
            inbox_messages(&conn, &bob.agent_id, true, 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn notify_without_file_reads_inbox_and_marks_messages_read() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "alice");
        let bob = named_test_session(&conn, "bob-notify-session", "bob");
        cmd_send(
            &conn,
            SendArgs {
                session: "bob".into(),
                project: None,
                message: vec!["notify me".into()],
                as_agent: Some("alice".into()),
                team: None,
                stdin: false,
                file: None,
                as_context: false,
                git_diff: false,
                cmd: None,
                subject: None,
                thread: None,
                context: None,
                message_text: None,
                json: false,
            },
            "message",
        )
        .unwrap();
        cmd_notify(
            &conn,
            NotifyArgs {
                project: None,
                runtime: Some(Runtime::Shell),
                session_id: Some("bob-notify-session".into()),
                hook: false,
                json: false,
            },
        )
        .unwrap();
        assert!(
            inbox_messages(&conn, &bob.agent_id, true, 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn hook_response_json_blocks_when_messages_exist() {
        let response = hook_response_json("new handoff message");
        assert_eq!(response["decision"], "block");
        assert!(
            response["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("new handoff message")
        );
    }

    #[test]
    fn hook_response_json_continues_when_no_messages_exist() {
        let response = hook_response_json("");
        assert_eq!(response["continue"], true);
        assert_eq!(response["systemMessage"], "handoff: no new messages");
    }

    #[test]
    fn inbox_peek_does_not_mark_messages_read() {
        let (_dir, conn) = test_conn();
        current_test_session(&conn, "alice");
        let bob = named_test_session(&conn, "bob-peek-session", "bob");
        cmd_send(
            &conn,
            SendArgs {
                session: "bob".into(),
                project: None,
                message: vec!["peek me".into()],
                as_agent: Some("alice".into()),
                team: None,
                stdin: false,
                file: None,
                as_context: false,
                git_diff: false,
                cmd: None,
                subject: None,
                thread: None,
                context: None,
                message_text: None,
                json: false,
            },
            "message",
        )
        .unwrap();
        cmd_inbox(
            &conn,
            InboxArgs {
                as_agent: None,
                project: None,
                session_id: Some("bob-peek-session".into()),
                unread: true,
                all: false,
                peek: true,
                no_mark_read: false,
                limit: 10,
                json: false,
            },
        )
        .unwrap();
        assert_eq!(
            inbox_messages(&conn, &bob.agent_id, true, 10)
                .unwrap()
                .len(),
            1
        );
        cmd_inbox(
            &conn,
            InboxArgs {
                as_agent: None,
                project: None,
                session_id: Some("bob-peek-session".into()),
                unread: true,
                all: false,
                peek: false,
                no_mark_read: false,
                limit: 10,
                json: false,
            },
        )
        .unwrap();
        assert!(
            inbox_messages(&conn, &bob.agent_id, true, 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn mode_turn_writes_codex_hook_and_off_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_path_buf();
        let (_db_dir, conn) = test_conn();
        cmd_mode(
            &conn,
            ModeArgs {
                mode: Some(DeliveryMode::Turn),
                runtime: Some(Runtime::Codex),
                project: Some(project.clone()),
                json: false,
            },
        )
        .unwrap();
        let hook_path = project.join(".codex/hooks.json");
        let content = fs::read_to_string(&hook_path).unwrap();
        assert!(content.contains("handoff"));
        assert!(content.contains("notify"));
        assert!(content.contains("--hook"));
        assert!(content.contains("--project"));

        cmd_mode(
            &conn,
            ModeArgs {
                mode: Some(DeliveryMode::Off),
                runtime: Some(Runtime::Codex),
                project: Some(project),
                json: false,
            },
        )
        .unwrap();
        assert!(!hook_path.exists());
    }

    #[test]
    fn mode_off_removes_generated_hook_without_handoff_binary_name() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_path_buf();
        let hook_path = project.join(".codex/hooks.json");
        fs::create_dir_all(hook_path.parent().unwrap()).unwrap();
        fs::write(
            &hook_path,
            format!(
                r#"{{"hooks":{{"Stop":[{{"hooks":[{{"type":"command","command":"'/tmp/bin/agent' inbox --json --project '{}'"}}]}}]}}}}"#,
                project.display()
            ),
        )
        .unwrap();
        let (_db_dir, conn) = test_conn();

        cmd_mode(
            &conn,
            ModeArgs {
                mode: Some(DeliveryMode::Off),
                runtime: Some(Runtime::Codex),
                project: Some(project),
                json: false,
            },
        )
        .unwrap();

        assert!(!hook_path.exists());
    }

    #[test]
    fn mode_monitor_writes_claude_session_start_hook() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_path_buf();
        let (_db_dir, conn) = test_conn();
        cmd_mode(
            &conn,
            ModeArgs {
                mode: Some(DeliveryMode::Monitor),
                runtime: Some(Runtime::ClaudeCode),
                project: Some(project.clone()),
                json: false,
            },
        )
        .unwrap();
        let hook_path = project.join(".claude/settings.local.json");
        let content = fs::read_to_string(&hook_path).unwrap();
        assert!(content.contains("SessionStart"));
        assert!(content.contains("monitor --instruction"));
        assert!(!content.contains("\"Stop\""));

        cmd_mode(
            &conn,
            ModeArgs {
                mode: Some(DeliveryMode::Both),
                runtime: Some(Runtime::ClaudeCode),
                project: Some(project.clone()),
                json: false,
            },
        )
        .unwrap();
        let content = fs::read_to_string(&hook_path).unwrap();
        assert!(content.contains("SessionStart"));
        assert!(content.contains("\"Stop\""));
        assert!(content.contains("notify --hook --json --project"));
    }

    #[test]
    fn mode_off_preserves_unrelated_json_settings() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_path_buf();
        let hook_path = project.join(".codex/hooks.json");
        fs::create_dir_all(hook_path.parent().unwrap()).unwrap();
        fs::write(&hook_path, r#"{"theme":"dark"}"#).unwrap();
        let (_db_dir, conn) = test_conn();
        cmd_mode(
            &conn,
            ModeArgs {
                mode: Some(DeliveryMode::Turn),
                runtime: Some(Runtime::Codex),
                project: Some(project.clone()),
                json: false,
            },
        )
        .unwrap();
        cmd_mode(
            &conn,
            ModeArgs {
                mode: Some(DeliveryMode::Off),
                runtime: Some(Runtime::Codex),
                project: Some(project),
                json: false,
            },
        )
        .unwrap();
        let content = fs::read_to_string(&hook_path).unwrap();
        assert!(content.contains("theme"));
        assert!(!content.contains("handoff"));
    }

    #[test]
    fn error_hints_cover_common_operator_failures() {
        assert!(
            error_hint("multiple_identities: use --as <agent>")
                .unwrap()
                .contains("handoff sessions")
        );
        assert!(
            error_hint("context_capture_failed: provide input")
                .unwrap()
                .contains("handoff context create")
        );
        assert!(
            error_hint("job_not_finished: job has no result yet")
                .unwrap()
                .contains("handoff status")
        );
    }
}
