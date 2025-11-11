use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use colored::*;
use ledger::types::{ModelProfile, TokenKind, TransactionKind};
use reqwest::Client;
use serde_json::Value;
use std::str::FromStr;

#[derive(Parser)]
#[command(name = "gpang", author, version, about = "GPANG Network CLI", long_about = None)]
struct Cli {
    /// RPC endpoint (Axum server)
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    rpc: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Provider management operations
    Provider(ProviderCommand),
    /// Node management operations
    Node(NodeCommand),
    /// Submit and inspect tasks
    Task(TaskCommand),
    /// Account level token operations
    Account(AccountCommand),
    /// Simple network status helpers
    Status,
}

#[derive(Subcommand)]
enum ProviderCommand {
    /// Register or update a GPU provider
    UpsertGpu(UpsertGpuArgs),
    /// Replace provider model profiles
    SetModels(SetModelsArgs),
    /// Send a heartbeat to mark the provider online
    Heartbeat { provider_id: String },
}

#[derive(Args)]
struct UpsertGpuArgs {
    #[arg(long)]
    id: String,
    #[arg(long)]
    endpoint: String,
    #[arg(long)]
    owner: String,
    #[arg(long)]
    region: String,
    #[arg(long)]
    gpu_model: String,
    #[arg(long)]
    vram_gb: u64,
    #[arg(long, default_value_t = 10)]
    bandwidth_gbps: u64,
    #[arg(long, default_value_t = true)]
    cuda: bool,
    #[arg(long, default_value_t = false)]
    rocm: bool,
    #[arg(long, default_value_t = 0)]
    reputation: u64,
    #[arg(long, default_value_t = 1.0)]
    stake_boost: f64,
    #[arg(long, default_value_t = 0)]
    low_tier_discount_bps: u64,
    /// Pricing envelope JSON (optional)
    #[arg(long)]
    pricing: Option<String>,
    /// Availability JSON (optional)
    #[arg(long)]
    availability: Option<String>,
}

#[derive(Args)]
struct SetModelsArgs {
    #[arg(long)]
    id: String,
    /// JSON array of ModelProfile objects
    #[arg(long)]
    profiles: String,
}

#[derive(Subcommand)]
enum NodeCommand {
    /// Register a node for scheduling
    Register(RegisterNodeArgs),
    /// Toggle node online/offline
    Status(UpdateNodeStatusArgs),
}

#[derive(Args)]
struct RegisterNodeArgs {
    #[arg(long)]
    id: String,
    #[arg(long)]
    owner: String,
    #[arg(long)]
    gpu_model: String,
    #[arg(long)]
    region: String,
    /// JSON ModelProfile payload
    #[arg(long)]
    llm_profile: String,
}

#[derive(Args)]
struct UpdateNodeStatusArgs {
    #[arg(long)]
    node_id: String,
    #[arg(long)]
    online: bool,
}

#[derive(Subcommand)]
enum TaskCommand {
    /// Submit a distributed inference task
    Submit(SubmitTaskArgs),
    /// Submit a task-proof commitment for consensus
    Proof(TaskProofArgs),
}

#[derive(Args)]
struct SubmitTaskArgs {
    #[arg(long)]
    owner: String,
    #[arg(long)]
    content_hash: String,
    #[arg(long)]
    total_tokens: u64,
    /// JSON ModelProfile payload
    #[arg(long)]
    target_profile: String,
    /// JSON array of provider identifiers
    #[arg(long)]
    providers: String,
    /// Optional region preference to bias scheduling
    #[arg(long)]
    preferred_region: Option<String>,
}

#[derive(Args)]
struct TaskProofArgs {
    #[arg(long)]
    round: u64,
    #[arg(long)]
    task_id: String,
    #[arg(long)]
    segment_id: String,
    #[arg(long)]
    provider_id: String,
    #[arg(long)]
    latency_ms: u64,
    #[arg(long)]
    throughput_tok_s: u64,
    #[arg(long)]
    tokens_processed: u64,
    #[arg(long)]
    region: String,
    #[arg(long)]
    signature: String,
}

#[derive(Subcommand)]
enum AccountCommand {
    /// Mint new tokens to an account
    Mint {
        to: String,
        token: String,
        amount: u64,
    },
    /// Transfer tokens between accounts
    Transfer {
        from: String,
        to: String,
        token: String,
        amount: u64,
    },
    /// Stake AIA into the consensus set
    Stake { owner: String, amount: u64 },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::builder().build()?;

    match cli.command {
        Commands::Provider(cmd) => handle_provider(&client, &cli.rpc, cmd).await?,
        Commands::Node(cmd) => handle_node(&client, &cli.rpc, cmd).await?,
        Commands::Task(cmd) => handle_task(&client, &cli.rpc, cmd).await?,
        Commands::Account(cmd) => handle_account(&client, &cli.rpc, cmd).await?,
        Commands::Status => print_status(&client, &cli.rpc).await?,
    }

    Ok(())
}

async fn handle_provider(client: &Client, rpc: &str, cmd: ProviderCommand) -> Result<()> {
    match cmd {
        ProviderCommand::UpsertGpu(args) => {
            let pricing: Option<Value> = args
                .pricing
                .as_ref()
                .map(|p| serde_json::from_str(p))
                .transpose()
                .context("invalid pricing JSON")?;
            let availability: Option<Value> = args
                .availability
                .as_ref()
                .map(|a| serde_json::from_str(a))
                .transpose()
                .context("invalid availability JSON")?;
            let body = serde_json::json!({
                "id": args.id,
                "endpoint": args.endpoint,
                "owner": args.owner,
                "region": args.region,
                "gpu_model": args.gpu_model,
                "vram_gb": args.vram_gb,
                "bandwidth_gbps": args.bandwidth_gbps,
                "cuda": args.cuda,
                "rocm": args.rocm,
                "reputation": args.reputation,
                "stake_boost": args.stake_boost,
                "low_tier_discount_bps": args.low_tier_discount_bps,
                "pricing": pricing,
                "availability": availability,
            });
            let res = client
                .post(format!("{}/provider/upsert", rpc))
                .json(&body)
                .send()
                .await?
                .error_for_status()?;
            println!("{}", "Provider updated".green());
            println!("{}", res.text().await?);
        }
        ProviderCommand::SetModels(args) => {
            let profiles: Vec<ModelProfile> =
                serde_json::from_str(&args.profiles).context("invalid model profile JSON")?;
            let body = serde_json::json!({
                "provider_id": args.id,
                "profiles": profiles,
            });
            client
                .post(format!("{}/provider/models", rpc))
                .json(&body)
                .send()
                .await?
                .error_for_status()?;
            println!("{}", "Model profiles updated".green());
        }
        ProviderCommand::Heartbeat { provider_id } => {
            let body = serde_json::json!({ "provider_id": provider_id });
            client
                .post(format!("{}/provider/heartbeat", rpc))
                .json(&body)
                .send()
                .await?
                .error_for_status()?;
            println!("{}", "Heartbeat recorded".green());
        }
    }
    Ok(())
}

async fn handle_node(client: &Client, rpc: &str, cmd: NodeCommand) -> Result<()> {
    match cmd {
        NodeCommand::Register(args) => {
            let profile: ModelProfile =
                serde_json::from_str(&args.llm_profile).context("invalid llm_profile JSON")?;
            let body = serde_json::json!({
                "id": args.id,
                "owner": args.owner,
                "gpu_model": args.gpu_model,
                "region": args.region,
                "llm_profile": profile,
            });
            let res = client
                .post(format!("{}/node/register", rpc))
                .json(&body)
                .send()
                .await?
                .error_for_status()?;
            println!("{}", "Node registered".green());
            println!("{}", res.text().await?);
        }
        NodeCommand::Status(args) => {
            let body = serde_json::json!({
                "node_id": args.node_id,
                "online": args.online,
            });
            client
                .post(format!("{}/node/status", rpc))
                .json(&body)
                .send()
                .await?
                .error_for_status()?;
            println!("{}", "Node status updated".green());
        }
    }
    Ok(())
}

async fn handle_task(client: &Client, rpc: &str, cmd: TaskCommand) -> Result<()> {
    match cmd {
        TaskCommand::Submit(args) => {
            let target_profile: ModelProfile = serde_json::from_str(&args.target_profile)
                .context("invalid target profile JSON")?;
            let providers: Vec<String> =
                serde_json::from_str(&args.providers).context("invalid providers JSON")?;
            let body = serde_json::json!({
                "owner": args.owner,
                "content_hash": args.content_hash,
                "total_tokens": args.total_tokens,
                "target_profile": target_profile,
                "providers": providers,
                "preferred_region": args.preferred_region,
            });
            let res = client
                .post(format!("{}/task/submit", rpc))
                .json(&body)
                .send()
                .await?
                .error_for_status()?;
            println!("{}", "Task submitted".green());
            println!("{}", res.text().await?);
        }
        TaskCommand::Proof(args) => {
            let body = serde_json::json!({
                "round": args.round,
                "task_id": args.task_id,
                "segment_id": args.segment_id,
                "provider_id": args.provider_id,
                "latency_ms": args.latency_ms,
                "throughput_tok_s": args.throughput_tok_s,
                "tokens_processed": args.tokens_processed,
                "region": args.region,
                "signature": args.signature,
            });
            let res = client
                .post(format!("{}/consensus/task-proof", rpc))
                .json(&body)
                .send()
                .await?
                .error_for_status()?;
            println!("{}", "Task-proof submitted".green());
            println!("{}", res.text().await?);
        }
    }
    Ok(())
}

async fn handle_account(client: &Client, rpc: &str, cmd: AccountCommand) -> Result<()> {
    match cmd {
        AccountCommand::Mint { to, token, amount } => {
            let token = parse_token(&token)?;
            send_transaction(
                client,
                rpc,
                TransactionKind::Mint {
                    to,
                    token,
                    amount,
                    reason: Some("cli_mint".into()),
                },
            )
            .await?;
        }
        AccountCommand::Transfer {
            from,
            to,
            token,
            amount,
        } => {
            let token = parse_token(&token)?;
            send_transaction(
                client,
                rpc,
                TransactionKind::Transfer {
                    from,
                    to,
                    token,
                    amount,
                    memo: Some("cli_transfer".into()),
                },
            )
            .await?;
        }
        AccountCommand::Stake { owner, amount } => {
            send_transaction(client, rpc, TransactionKind::Stake { owner, amount }).await?;
        }
    }
    Ok(())
}

async fn print_status(client: &Client, rpc: &str) -> Result<()> {
    let res = client
        .get(format!("{}/", rpc))
        .send()
        .await?
        .error_for_status()?;
    let text = res.text().await?;
    println!("{}", "Network summary".cyan());
    println!("{}", text);
    Ok(())
}

async fn send_transaction(client: &Client, rpc: &str, kind: TransactionKind) -> Result<()> {
    let body = serde_json::json!({ "kind": kind });
    let res = client
        .post(format!("{}/tx", rpc))
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    println!("{}", "Transaction submitted".green());
    println!("{}", res.text().await?);
    Ok(())
}

fn parse_token(token: &str) -> Result<TokenKind> {
    TokenKind::from_str(&token.to_ascii_uppercase())
        .map_err(|_| anyhow!("unknown token: {}", token))
}

impl FromStr for TokenKind {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "AIA" => Ok(TokenKind::AIA),
            "WORK" => Ok(TokenKind::WORK),
            "STOR" => Ok(TokenKind::STOR),
            other => Err(other.to_string()),
        }
    }
}
