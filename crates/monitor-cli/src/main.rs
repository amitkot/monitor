use std::process;

use clap::{Parser, Subcommand};
use uuid::Uuid;

mod client;

// ---------------------------------------------------------------------------
// CLI structure
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "monitor", about = "Monitor CLI - send updates and manage state")]
struct Cli {
    /// Server URL
    #[arg(long, env = "MONITOR_SERVER", default_value = "http://127.0.0.1:3000")]
    server: String,

    /// API bearer token
    #[arg(long, env = "MONITOR_TOKEN")]
    token: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage workstreams
    Workstream(WorkstreamCmd),
    /// Manage tasks
    Task(TaskCmd),
    /// Send updates
    Update(UpdateCmd),
}

// ---------------------------------------------------------------------------
// Workstream commands
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct WorkstreamCmd {
    #[command(subcommand)]
    action: WorkstreamAction,
}

#[derive(Subcommand)]
enum WorkstreamAction {
    /// Create a new workstream
    Create {
        /// Name of the workstream
        name: String,
        /// Optional JSON metadata
        #[arg(long)]
        metadata: Option<String>,
    },
    /// List workstreams
    List {
        /// Include archived workstreams
        #[arg(long, default_value_t = false)]
        include_archived: bool,
    },
    /// Update an existing workstream
    Update {
        /// Workstream ID
        id: Uuid,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// New status (active or archived)
        #[arg(long)]
        status: Option<String>,
        /// New JSON metadata
        #[arg(long)]
        metadata: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Task commands
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct TaskCmd {
    #[command(subcommand)]
    action: TaskAction,
}

#[derive(Subcommand)]
enum TaskAction {
    /// Create a new task
    Create {
        /// Name of the task
        name: String,
        /// Workstream ID this task belongs to
        #[arg(long)]
        workstream: Uuid,
        /// Optional JSON metadata
        #[arg(long)]
        metadata: Option<String>,
    },
    /// List tasks
    List {
        /// Filter by workstream ID
        #[arg(long)]
        workstream: Option<Uuid>,
        /// Filter by status (active, blocked, done, cancelled)
        #[arg(long)]
        status: Option<String>,
    },
    /// Update an existing task
    Update {
        /// Task ID
        id: Uuid,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// New status (active, blocked, done, cancelled)
        #[arg(long)]
        status: Option<String>,
        /// Summary text
        #[arg(long)]
        summary: Option<String>,
        /// Summary source
        #[arg(long)]
        summary_source: Option<String>,
        /// New JSON metadata
        #[arg(long)]
        metadata: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Update commands
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct UpdateCmd {
    #[command(subcommand)]
    action: UpdateAction,
}

#[derive(Subcommand)]
enum UpdateAction {
    /// Send a manual update
    Manual {
        /// Task ID to attach the update to
        #[arg(long)]
        task: Uuid,
        /// Update message text
        #[arg(long)]
        message: String,
        /// Level: info, warn, or error
        #[arg(long)]
        level: Option<String>,
        /// Kind of update
        #[arg(long)]
        kind: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// JSON data payload
        #[arg(long)]
        data: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_json_opt(raw: &Option<String>, field: &str) -> Result<Option<serde_json::Value>, String> {
    match raw {
        None => Ok(None),
        Some(s) => serde_json::from_str(s)
            .map(Some)
            .map_err(|e| format!("invalid JSON for --{field}: {e}")),
    }
}

fn parse_workstream_status(s: &str) -> Result<monitor_common::WorkstreamStatus, String> {
    match s {
        "active" => Ok(monitor_common::WorkstreamStatus::Active),
        "archived" => Ok(monitor_common::WorkstreamStatus::Archived),
        other => Err(format!(
            "invalid workstream status '{other}': expected 'active' or 'archived'"
        )),
    }
}

fn parse_task_status(s: &str) -> Result<monitor_common::TaskStatus, String> {
    match s {
        "active" => Ok(monitor_common::TaskStatus::Active),
        "blocked" => Ok(monitor_common::TaskStatus::Blocked),
        "done" => Ok(monitor_common::TaskStatus::Done),
        "cancelled" => Ok(monitor_common::TaskStatus::Cancelled),
        other => Err(format!(
            "invalid task status '{other}': expected 'active', 'blocked', 'done', or 'cancelled'"
        )),
    }
}

fn parse_update_level(s: &str) -> Result<monitor_common::UpdateLevel, String> {
    match s {
        "info" => Ok(monitor_common::UpdateLevel::Info),
        "warn" => Ok(monitor_common::UpdateLevel::Warn),
        "error" => Ok(monitor_common::UpdateLevel::Error),
        other => Err(format!(
            "invalid update level '{other}': expected 'info', 'warn', or 'error'"
        )),
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let client = client::MonitorClient::new(cli.server, cli.token);

    let result = run(client, cli.command).await;
    match result {
        Ok(value) => {
            let formatted = serde_json::to_string_pretty(&value).unwrap_or_default();
            println!("{formatted}");
        }
        Err(msg) => {
            eprintln!("Error: {msg}");
            process::exit(1);
        }
    }
}

async fn run(
    client: client::MonitorClient,
    command: Commands,
) -> Result<serde_json::Value, String> {
    match command {
        Commands::Workstream(cmd) => run_workstream(client, cmd.action).await,
        Commands::Task(cmd) => run_task(client, cmd.action).await,
        Commands::Update(cmd) => run_update(client, cmd.action).await,
    }
}

// ---------------------------------------------------------------------------
// Workstream handlers
// ---------------------------------------------------------------------------

async fn run_workstream(
    client: client::MonitorClient,
    action: WorkstreamAction,
) -> Result<serde_json::Value, String> {
    use monitor_common::api::{CreateWorkstreamRequest, PatchWorkstreamRequest};

    match action {
        WorkstreamAction::Create { name, metadata } => {
            let metadata = parse_json_opt(&metadata, "metadata")?;
            let body = CreateWorkstreamRequest { name, metadata };
            client.post("/api/workstreams", &body).await
        }
        WorkstreamAction::List { include_archived } => {
            let mut query: Vec<(&str, String)> = Vec::new();
            if include_archived {
                query.push(("include_archived", "true".to_string()));
            }
            let pairs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
            client.get("/api/workstreams", &pairs).await
        }
        WorkstreamAction::Update {
            id,
            name,
            status,
            metadata,
        } => {
            let status = status.map(|s| parse_workstream_status(&s)).transpose()?;
            let metadata = parse_json_opt(&metadata, "metadata")?;
            let body = PatchWorkstreamRequest {
                name,
                status,
                metadata,
            };
            client.patch(&format!("/api/workstreams/{id}"), &body).await
        }
    }
}

// ---------------------------------------------------------------------------
// Task handlers
// ---------------------------------------------------------------------------

async fn run_task(
    client: client::MonitorClient,
    action: TaskAction,
) -> Result<serde_json::Value, String> {
    use monitor_common::api::{CreateTaskRequest, PatchTaskRequest};

    match action {
        TaskAction::Create {
            name,
            workstream,
            metadata,
        } => {
            let metadata = parse_json_opt(&metadata, "metadata")?;
            let body = CreateTaskRequest {
                workstream_id: workstream,
                name,
                metadata,
            };
            client.post("/api/tasks", &body).await
        }
        TaskAction::List { workstream, status } => {
            let mut query: Vec<(&str, String)> = Vec::new();
            if let Some(ws) = workstream {
                query.push(("workstream_id", ws.to_string()));
            }
            if let Some(s) = &status {
                // Validate the status value
                parse_task_status(s)?;
                query.push(("status", s.clone()));
            }
            let pairs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
            client.get("/api/tasks", &pairs).await
        }
        TaskAction::Update {
            id,
            name,
            status,
            summary,
            summary_source,
            metadata,
        } => {
            let status = status.map(|s| parse_task_status(&s)).transpose()?;
            let metadata = parse_json_opt(&metadata, "metadata")?;
            let body = PatchTaskRequest {
                name,
                workstream_id: None,
                status,
                summary_text: summary,
                summary_source,
                metadata,
            };
            client.patch(&format!("/api/tasks/{id}"), &body).await
        }
    }
}

// ---------------------------------------------------------------------------
// Update handlers
// ---------------------------------------------------------------------------

async fn run_update(
    client: client::MonitorClient,
    action: UpdateAction,
) -> Result<serde_json::Value, String> {
    use monitor_common::api::ManualUpdateRequest;

    match action {
        UpdateAction::Manual {
            task,
            message,
            level,
            kind,
            tags,
            data,
        } => {
            let level = level.map(|l| parse_update_level(&l)).transpose()?;
            let data = parse_json_opt(&data, "data")?;
            let tags = tags
                .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();
            let body = ManualUpdateRequest {
                task_id: task,
                message,
                level,
                kind,
                tags,
                data,
            };
            client.post("/api/updates/manual", &body).await
        }
    }
}
