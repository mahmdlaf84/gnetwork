use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use clap::{Args, Parser, Subcommand};
use colored::*;
use ledger::types::{
    ContractRuntime, ModelProfile, NodeHardware, NodeMetrics, NodeRole, TaskMode, TokenKind,
    TransactionKind,
};
use reqwest::Client;
use serde_json::{self, Value};
use std::{
    collections::HashMap,
    fs,
    hash::{Hash, Hasher},
    process::Command,
    str::FromStr,
    thread::sleep,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use sysinfo::{CpuExt, DiskExt, LoadAvg, NetworkExt, NetworksExt, System, SystemExt};

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
    /// Manage on-chain smart contracts
    Contract(ContractCommand),
    /// Manage custom and platform tokens
    Token(TokenCommand),
    /// Treasury movements
    Treasury(TreasuryCommand),
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
    /// List nodes, optionally filtered by owner account
    List {
        #[arg(long)]
        owner: Option<String>,
    },
    /// Show a single node record
    Show {
        #[arg(long)]
        node_id: String,
    },
    /// Display a node's cumulative earnings
    Earnings {
        #[arg(long)]
        node_id: String,
    },
}

#[derive(Args)]
struct RegisterNodeArgs {
    #[arg(long)]
    owner: String,
    #[arg(long)]
    region: String,
    /// JSON ModelProfile payload
    #[arg(long)]
    llm_profile: String,
    /// Optional override for the derived hardware fingerprint
    #[arg(long)]
    fingerprint: Option<String>,
    /// Node role: validator | compute | scheduler | assignment
    #[arg(long, default_value = "compute")]
    role: String,
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
    /// Task execution mode (batch or chat)
    #[arg(long, default_value = "batch")]
    mode: String,
    /// Prompt content when submitting chat mode tasks
    #[arg(long)]
    chat_prompt: Option<String>,
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
    #[arg(long)]
    node_id: Option<String>,
    #[arg(long)]
    output_digest: Option<String>,
    #[arg(long)]
    chat_response: Option<String>,
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

#[derive(Subcommand)]
enum ContractCommand {
    /// Deploy a new smart contract to the chain
    Deploy(DeployContractArgs),
    /// Execute a contract method and record the payload
    Execute(ExecuteContractArgs),
}

#[derive(Args)]
struct DeployContractArgs {
    #[arg(long)]
    owner: String,
    #[arg(long)]
    name: String,
    /// Inline source code for the contract (mutually exclusive with code_path)
    #[arg(long)]
    code: Option<String>,
    /// Path to a file containing the contract source
    #[arg(long)]
    code_path: Option<String>,
    /// Optional explicit identifier; otherwise the ledger allocates one
    #[arg(long)]
    contract_id: Option<String>,
    /// Optional metadata payload stored alongside the contract
    #[arg(long)]
    metadata: Option<String>,
    /// Target runtime for the contract (native or solana)
    #[arg(long, default_value = "native")]
    runtime: String,
    /// Solana program identifier when deploying Solana BPF contracts
    #[arg(long)]
    program_id: Option<String>,
    /// Path to a compiled Solana shared object for base64 encoding
    #[arg(long)]
    bytecode_path: Option<String>,
    /// Inline base64-encoded bytecode payload for Solana contracts
    #[arg(long)]
    bytecode: Option<String>,
}

#[derive(Args)]
struct ExecuteContractArgs {
    #[arg(long)]
    contract_id: String,
    #[arg(long)]
    caller: String,
    #[arg(long)]
    method: String,
    /// JSON payload passed into the contract runtime
    #[arg(long)]
    payload: Option<String>,
}

#[derive(Subcommand)]
enum TokenCommand {
    /// Create a new fungible token definition
    Create(CreateTokenArgs),
    /// Mint additional custom token supply
    Mint(MintTokenArgs),
    /// Transfer balances of a custom token
    Transfer(TransferCustomTokenArgs),
}

#[derive(Args)]
struct CreateTokenArgs {
    #[arg(long)]
    symbol: String,
    #[arg(long)]
    name: String,
    #[arg(long, default_value_t = 9)]
    decimals: u8,
    #[arg(long, default_value_t = 0)]
    initial_supply: u64,
    #[arg(long)]
    owner: String,
    #[arg(long, default_value_t = false)]
    platform: bool,
    #[arg(long)]
    contract_id: Option<String>,
}

#[derive(Args)]
struct MintTokenArgs {
    #[arg(long)]
    symbol: String,
    #[arg(long)]
    to: String,
    #[arg(long)]
    amount: u64,
    #[arg(long)]
    authority: String,
}

#[derive(Args)]
struct TransferCustomTokenArgs {
    #[arg(long)]
    symbol: String,
    #[arg(long)]
    from: String,
    #[arg(long)]
    to: String,
    #[arg(long)]
    amount: u64,
}

#[derive(Subcommand)]
enum TreasuryCommand {
    /// Move user funds into the treasury reserves
    Deposit {
        #[arg(long)]
        from: String,
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        amount: u64,
    },
    /// Release treasury funds to an account
    Withdraw {
        #[arg(long)]
        to: String,
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        amount: u64,
        #[arg(long)]
        authority: String,
    },
}

#[derive(Debug, Default)]
struct GpuSnapshot {
    vendor: String,
    model: String,
    memory_total_mb: u64,
    memory_used_mb: u64,
    usage_pct: f32,
}

fn capture_node_state() -> (NodeHardware, NodeMetrics) {
    let mut system = System::new_all();
    system.refresh_all();

    let initial_networks: HashMap<String, (u64, u64)> = system
        .networks()
        .iter()
        .map(|(iface, data)| {
            (
                iface.clone(),
                (data.total_received(), data.total_transmitted()),
            )
        })
        .collect();

    sleep(Duration::from_millis(200));
    system.refresh_cpu();
    system.refresh_memory();
    system.refresh_networks();
    system.refresh_disks();

    let load_avg: LoadAvg = system.load_average();
    let total_memory = system.total_memory();
    let used_memory = system.used_memory();
    let memory_usage_pct = if total_memory > 0 {
        (used_memory as f32 / total_memory as f32) * 100.0
    } else {
        0.0
    };

    let mut total_disk_bytes: u128 = 0;
    let mut used_disk_bytes: u128 = 0;
    for disk in system.disks() {
        let total = disk.total_space() as u128;
        let available = disk.available_space() as u128;
        total_disk_bytes += total;
        used_disk_bytes += total.saturating_sub(available);
    }
    let disk_usage_pct = if total_disk_bytes > 0 {
        (used_disk_bytes as f64 / total_disk_bytes as f64 * 100.0) as f32
    } else {
        0.0
    };

    let mut rx_bytes: u64 = 0;
    let mut tx_bytes: u64 = 0;
    for (iface, data) in system.networks() {
        if let Some((start_rx, start_tx)) = initial_networks.get(iface) {
            rx_bytes = rx_bytes.saturating_add(data.total_received().saturating_sub(*start_rx));
            tx_bytes = tx_bytes.saturating_add(data.total_transmitted().saturating_sub(*start_tx));
        }
    }
    let interval_secs = 0.2f32;
    let network_rx_mbps = (rx_bytes as f32 * 8.0) / (interval_secs * 1_000_000.0);
    let network_tx_mbps = (tx_bytes as f32 * 8.0) / (interval_secs * 1_000_000.0);

    let gpu = detect_gpu_info();
    let cpu_brand = system.global_cpu_info().brand().to_string();
    let cpu_threads = system.cpus().len() as u32;
    let cpu_cores = system.physical_core_count().unwrap_or(cpu_threads as usize) as u32;
    let memory_total_mb = (total_memory / 1024) as u64;
    let os_version = system
        .long_os_version()
        .or_else(|| system.name())
        .unwrap_or_else(|| std::env::consts::OS.to_string());

    let hardware = NodeHardware {
        cpu_model: cpu_brand,
        cpu_cores,
        cpu_threads,
        memory_total_mb,
        gpu_vendor: if gpu.vendor.is_empty() {
            "Unknown".to_string()
        } else {
            gpu.vendor.clone()
        },
        gpu_model: if gpu.model.is_empty() {
            "Unknown".to_string()
        } else {
            gpu.model.clone()
        },
        gpu_vram_mb: gpu.memory_total_mb,
        os: os_version,
    };

    let metrics = NodeMetrics {
        timestamp: now_ms(),
        cpu_usage_pct: system.global_cpu_info().cpu_usage(),
        memory_usage_pct,
        gpu_usage_pct: gpu.usage_pct,
        machine_load_one: load_avg.one as f32,
        machine_load_five: load_avg.five as f32,
        machine_load_fifteen: load_avg.fifteen as f32,
        disk_usage_pct,
        disk_read_mbps: 0.0,
        disk_write_mbps: 0.0,
        network_rx_mbps,
        network_tx_mbps,
        gpu_memory_used_mb: gpu.memory_used_mb,
        gpu_memory_total_mb: gpu.memory_total_mb,
    };

    (hardware, metrics)
}

fn capture_runtime_metrics() -> NodeMetrics {
    let (_, metrics) = capture_node_state();
    metrics
}

fn derive_fingerprint(hardware: &NodeHardware, owner: &str, override_fp: Option<String>) -> String {
    if let Some(fp) = override_fp {
        return fp;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hardware.cpu_model.hash(&mut hasher);
    hardware.cpu_cores.hash(&mut hasher);
    hardware.cpu_threads.hash(&mut hasher);
    hardware.memory_total_mb.hash(&mut hasher);
    hardware.gpu_vendor.hash(&mut hasher);
    hardware.gpu_model.hash(&mut hasher);
    hardware.gpu_vram_mb.hash(&mut hasher);
    hardware.os.hash(&mut hasher);
    owner.hash(&mut hasher);
    format!("0x{:016x}", hasher.finish())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn detect_gpu_info() -> GpuSnapshot {
    if let Some(output) = run_command_output(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total,memory.used,utilization.gpu",
            "--format=csv,noheader,nounits",
        ],
    ) {
        if let Some(line) = output.lines().next() {
            let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
            if parts.len() >= 4 {
                let total = parts[1]
                    .parse::<f64>()
                    .map(|v| v.round() as u64)
                    .unwrap_or_default();
                let used = parts[2]
                    .parse::<f64>()
                    .map(|v| v.round() as u64)
                    .unwrap_or_default();
                let usage = parts[3].parse::<f32>().unwrap_or_default();
                return GpuSnapshot {
                    vendor: "NVIDIA".to_string(),
                    model: parts[0].to_string(),
                    memory_total_mb: total,
                    memory_used_mb: used,
                    usage_pct: usage,
                };
            }
        }
    }

    if let Some(output) = run_command_output("system_profiler", &["SPDisplaysDataType", "-json"]) {
        if let Ok(value) = serde_json::from_str::<Value>(&output) {
            if let Some(array) = value.get("SPDisplaysDataType").and_then(|v| v.as_array()) {
                if let Some(entry) = array.first() {
                    let vendor = entry
                        .get("spdisplays_vendor")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Apple");
                    let model = entry
                        .get("spdisplays_chipset-model")
                        .or_else(|| entry.get("_name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("Apple GPU");
                    let memory_total_mb = entry
                        .get("spdisplays_vram")
                        .and_then(|v| v.as_str())
                        .map(parse_size_to_mb)
                        .unwrap_or_default();
                    return GpuSnapshot {
                        vendor: vendor.to_string(),
                        model: model.to_string(),
                        memory_total_mb,
                        memory_used_mb: 0,
                        usage_pct: 0.0,
                    };
                }
            }
        }
    }

    if let Some(output) = run_command_output(
        "rocm-smi",
        &[
            "--showproductname",
            "--showmeminfo",
            "vram",
            "--showuse",
            "--json",
        ],
    ) {
        if let Ok(value) = serde_json::from_str::<Value>(&output) {
            if let Some(obj) = value.as_object() {
                for entry in obj.values() {
                    let model = entry
                        .get("Card SKU")
                        .or_else(|| entry.get("card_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("AMD GPU")
                        .to_string();
                    let memory_total_mb = entry
                        .get("VRAM Total Memory (B)")
                        .or_else(|| entry.get("vram_total"))
                        .and_then(|v| v.as_str().or_else(|| v.as_i64().map(|n| n.to_string())))
                        .map(|s| parse_size_to_mb(&s))
                        .unwrap_or_default();
                    let memory_used_mb = entry
                        .get("VRAM Used Memory (B)")
                        .or_else(|| entry.get("vram_used"))
                        .and_then(|v| v.as_str().or_else(|| v.as_i64().map(|n| n.to_string())))
                        .map(|s| parse_size_to_mb(&s))
                        .unwrap_or_default();
                    let usage_pct = entry
                        .get("GPU use (%)")
                        .or_else(|| entry.get("gpu_use"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or_default() as f32;
                    return GpuSnapshot {
                        vendor: "AMD".to_string(),
                        model,
                        memory_total_mb,
                        memory_used_mb,
                        usage_pct,
                    };
                }
            }
        }
    }

    GpuSnapshot::default()
}

fn run_command_output(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|out| out.trim().to_string())
        .filter(|out| !out.is_empty())
}

fn parse_size_to_mb(value: &str) -> u64 {
    let cleaned = value.trim().to_lowercase();
    if cleaned.is_empty() {
        return 0;
    }
    if cleaned.chars().all(|c| c.is_ascii_digit()) {
        let bytes: f64 = cleaned.parse().unwrap_or(0.0);
        return (bytes / (1024.0 * 1024.0)).round() as u64;
    }
    let mut numeric = String::new();
    for ch in cleaned.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            numeric.push(ch);
        } else if !numeric.is_empty() {
            break;
        }
    }
    let number: f64 = numeric.parse().unwrap_or(0.0);
    if cleaned.contains("tb") {
        (number * 1024.0 * 1024.0).round() as u64
    } else if cleaned.contains("gb") {
        (number * 1024.0).round() as u64
    } else if cleaned.contains("mb") {
        number.round() as u64
    } else if cleaned.contains("kb") {
        (number / 1024.0).round() as u64
    } else if cleaned.contains('b') {
        (number / (1024.0 * 1024.0)).round() as u64
    } else {
        number.round() as u64
    }
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
        Commands::Contract(cmd) => handle_contract(&client, &cli.rpc, cmd).await?,
        Commands::Token(cmd) => handle_token(&client, &cli.rpc, cmd).await?,
        Commands::Treasury(cmd) => handle_treasury(&client, &cli.rpc, cmd).await?,
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
            let RegisterNodeArgs {
                owner,
                region,
                llm_profile,
                fingerprint,
                role,
            } = args;
            let profile: ModelProfile =
                serde_json::from_str(&llm_profile).context("invalid llm_profile JSON")?;
            let role = NodeRole::from_str(&role).context("invalid node role")?;
            let (hardware, metrics) = capture_node_state();
            let fingerprint = derive_fingerprint(&hardware, &owner, fingerprint);
            let body = serde_json::json!({
                "owner": owner,
                "region": region,
                "llm_profile": profile,
                "role": role,
                "hardware": hardware,
                "metrics": metrics,
                "fingerprint": fingerprint,
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
            let metrics = if args.online {
                Some(capture_runtime_metrics())
            } else {
                None
            };
            let body = if let Some(metrics) = metrics {
                serde_json::json!({
                    "node_id": args.node_id,
                    "online": args.online,
                    "metrics": metrics,
                })
            } else {
                serde_json::json!({
                    "node_id": args.node_id,
                    "online": args.online,
                })
            };
            client
                .post(format!("{}/node/status", rpc))
                .json(&body)
                .send()
                .await?
                .error_for_status()?;
            println!("{}", "Node status updated".green());
        }
        NodeCommand::List { owner } => {
            let url = if let Some(owner) = owner {
                format!("{}/accounts/{}/nodes", rpc, owner)
            } else {
                format!("{}/nodes", rpc)
            };
            let res = client.get(url).send().await?.error_for_status()?;
            let payload: Value = res.json().await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).context("failed to pretty print nodes")?
            );
        }
        NodeCommand::Show { node_id } => {
            let res = client
                .get(format!("{}/node/{}", rpc, node_id))
                .send()
                .await?
                .error_for_status()?;
            let payload: Value = res.json().await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).context("failed to pretty print node")?
            );
        }
        NodeCommand::Earnings { node_id } => {
            let res = client
                .get(format!("{}/node/{}/earnings", rpc, node_id))
                .send()
                .await?
                .error_for_status()?;
            let payload: Value = res.json().await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&payload)
                    .context("failed to pretty print node earnings")?
            );
        }
    }
    Ok(())
}

async fn handle_task(client: &Client, rpc: &str, cmd: TaskCommand) -> Result<()> {
    match cmd {
        TaskCommand::Submit(args) => {
            let SubmitTaskArgs {
                owner,
                content_hash,
                total_tokens,
                target_profile,
                providers,
                preferred_region,
                mode,
                chat_prompt,
            } = args;
            let target_profile: ModelProfile =
                serde_json::from_str(&target_profile).context("invalid target profile JSON")?;
            let providers: Vec<String> =
                serde_json::from_str(&providers).context("invalid providers JSON")?;
            let task_mode = parse_task_mode(&mode)?;
            if matches!(task_mode, TaskMode::Chat) && chat_prompt.is_none() {
                return Err(anyhow!("chat mode requires --chat-prompt"));
            }
            let mode_value = match task_mode {
                TaskMode::Batch => "batch",
                TaskMode::Chat => "chat",
            };
            let body = serde_json::json!({
                "owner": owner,
                "content_hash": content_hash,
                "total_tokens": total_tokens,
                "target_profile": target_profile,
                "providers": providers,
                "preferred_region": preferred_region,
                "mode": mode_value,
                "chat_prompt": chat_prompt,
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
            let TaskProofArgs {
                round,
                task_id,
                segment_id,
                provider_id,
                latency_ms,
                throughput_tok_s,
                tokens_processed,
                region,
                signature,
                node_id,
                output_digest,
                chat_response,
            } = args;
            let body = serde_json::json!({
                "round": round,
                "task_id": task_id,
                "segment_id": segment_id,
                "provider_id": provider_id,
                "latency_ms": latency_ms,
                "throughput_tok_s": throughput_tok_s,
                "tokens_processed": tokens_processed,
                "region": region,
                "signature": signature,
                "node_id": node_id,
                "output_digest": output_digest,
                "chat_response": chat_response,
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

async fn handle_contract(client: &Client, rpc: &str, cmd: ContractCommand) -> Result<()> {
    match cmd {
        ContractCommand::Deploy(mut args) => {
            let runtime = ContractRuntime::from_str(&args.runtime).map_err(|err| anyhow!(err))?;
            let mut source = match (args.code.take(), args.code_path.take()) {
                (Some(inline), None) => Some(inline),
                (None, Some(path)) => Some(
                    fs::read_to_string(&path)
                        .with_context(|| format!("failed to read contract file {}", path))?,
                ),
                (Some(_), Some(_)) => {
                    return Err(anyhow!(
                        "provide either --code or --code-path when deploying contracts"
                    ))
                }
                (None, None) => None,
            };
            let mut bytecode = match (args.bytecode.take(), args.bytecode_path.take()) {
                (Some(inline), None) => Some(inline),
                (None, Some(path)) => {
                    let bytes = fs::read(&path)
                        .with_context(|| format!("failed to read bytecode file {}", path))?;
                    Some(BASE64.encode(bytes))
                }
                (Some(_), Some(_)) => {
                    return Err(anyhow!(
                        "provide either --bytecode or --bytecode-path when deploying contracts"
                    ))
                }
                (None, None) => None,
            };
            let mut program_id = args.program_id.clone();

            match &runtime {
                ContractRuntime::Native => {
                    let src = source.as_ref().ok_or_else(|| {
                        anyhow!(
                            "native deployments require --code or --code-path to provide source"
                        )
                    })?;
                    if src.trim().is_empty() {
                        return Err(anyhow!("native contract code cannot be empty"));
                    }
                    program_id = None;
                    bytecode = None;
                }
                ContractRuntime::Solana => {
                    let provided = args
                        .program_id
                        .clone()
                        .ok_or_else(|| anyhow!("solana deployments require --program-id"))?;
                    let trimmed = provided.trim().to_string();
                    if trimmed.is_empty() {
                        return Err(anyhow!(
                            "solana deployments require a non-empty --program-id"
                        ));
                    }
                    program_id = Some(trimmed);
                    if bytecode.is_none() {
                        if let Some(src) = source.take() {
                            bytecode = Some(src);
                        }
                    }
                    let encoded = bytecode.as_ref().ok_or_else(|| {
                        anyhow!(
                            "solana deployments require --bytecode, --bytecode-path, or --code containing base64"
                        )
                    })?;
                    if encoded.trim().is_empty() {
                        return Err(anyhow!("solana bytecode cannot be empty"));
                    }
                    BASE64
                        .decode(encoded.as_bytes())
                        .context("solana bytecode must be valid base64")?;
                }
            }

            let kind = TransactionKind::DeployContract {
                owner: args.owner,
                contract_id: args.contract_id,
                name: args.name,
                code: source,
                metadata: args.metadata,
                runtime,
                program_id,
                bytecode_b64: bytecode,
            };
            send_transaction(client, rpc, kind).await?;
        }
        ContractCommand::Execute(args) => {
            let payload = if let Some(body) = args.payload {
                serde_json::from_str(&body).context("invalid JSON payload")?
            } else {
                Value::Null
            };
            let kind = TransactionKind::ExecuteContract {
                contract_id: args.contract_id,
                caller: args.caller,
                method: args.method,
                payload,
            };
            send_transaction(client, rpc, kind).await?;
        }
    }
    Ok(())
}

async fn handle_token(client: &Client, rpc: &str, cmd: TokenCommand) -> Result<()> {
    match cmd {
        TokenCommand::Create(args) => {
            let kind = TransactionKind::CreateToken {
                symbol: args.symbol.to_ascii_uppercase(),
                name: args.name,
                decimals: args.decimals,
                initial_supply: args.initial_supply,
                owner: args.owner,
                platform: args.platform,
                contract_id: args.contract_id,
            };
            send_transaction(client, rpc, kind).await?;
        }
        TokenCommand::Mint(args) => {
            let kind = TransactionKind::MintCustom {
                symbol: args.symbol.to_ascii_uppercase(),
                to: args.to,
                amount: args.amount,
                authority: args.authority,
            };
            send_transaction(client, rpc, kind).await?;
        }
        TokenCommand::Transfer(args) => {
            let kind = TransactionKind::TransferCustom {
                symbol: args.symbol.to_ascii_uppercase(),
                from: args.from,
                to: args.to,
                amount: args.amount,
            };
            send_transaction(client, rpc, kind).await?;
        }
    }
    Ok(())
}

async fn handle_treasury(client: &Client, rpc: &str, cmd: TreasuryCommand) -> Result<()> {
    match cmd {
        TreasuryCommand::Deposit {
            from,
            symbol,
            amount,
        } => {
            let kind = TransactionKind::TreasuryDeposit {
                from,
                symbol: symbol.to_ascii_uppercase(),
                amount,
            };
            send_transaction(client, rpc, kind).await?;
        }
        TreasuryCommand::Withdraw {
            to,
            symbol,
            amount,
            authority,
        } => {
            let kind = TransactionKind::TreasuryWithdraw {
                to,
                symbol: symbol.to_ascii_uppercase(),
                amount,
                authority,
            };
            send_transaction(client, rpc, kind).await?;
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

fn parse_task_mode(mode: &str) -> Result<TaskMode> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "batch" | "throughput" => Ok(TaskMode::Batch),
        "chat" | "conversation" => Ok(TaskMode::Chat),
        other => Err(anyhow!("unsupported task mode '{}'", other)),
    }
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
