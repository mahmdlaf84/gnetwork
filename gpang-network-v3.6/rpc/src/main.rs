use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::State, http::StatusCode, response::IntoResponse, routing::get, routing::post, Json,
    Router,
};
use clap::Parser;
use ledger::{current_timestamp, types::*, Ledger, NetworkSummary, DEFAULT_LEDGER_FILE};
use serde::Deserialize;
use tokio::{signal, sync::Mutex};
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Clone)]
struct AppState {
    ledger: Arc<Mutex<Ledger>>,
    mempool: Arc<Mutex<Vec<Transaction>>>,
}

#[derive(Parser, Debug)]
#[command(author, version, about = "GPANG Network RPC service", long_about = None)]
struct Args {
    /// Address to listen on (ip:port)
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: SocketAddr,
    /// Override the ledger persistence path
    #[arg(long)]
    ledger: Option<PathBuf>,
    /// Bootstrap a fresh deterministic testnet snapshot before starting
    #[arg(long)]
    testnet: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let ledger_path = args
        .ledger
        .or_else(|| std::env::var_os("LEDGER_FILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_LEDGER_FILE));

    let ledger = if args.testnet {
        info!(path = %ledger_path.display(), "initializing testnet ledger");
        Ledger::initialize_testnet(&ledger_path)?
    } else {
        info!(path = %ledger_path.display(), "loading ledger");
        Ledger::load_or_initialize(&ledger_path)?
    };
    let state = AppState {
        ledger: Arc::new(Mutex::new(ledger)),
        mempool: Arc::new(Mutex::new(Vec::new())),
    };

    spawn_consensus_loop(state.clone());

    let app = Router::new()
        .route("/", get(get_summary))
        .route("/blocks", get(list_blocks))
        .route("/accounts", get(list_accounts))
        .route("/providers", get(list_providers))
        .route("/nodes", get(list_nodes))
        .route("/tasks", get(list_tasks))
        .route("/treasury", get(get_treasury))
        .route("/tx", post(submit_transaction))
        .route("/provider/upsert", post(upsert_provider))
        .route("/provider/models", post(set_provider_models))
        .route("/provider/heartbeat", post(provider_heartbeat))
        .route("/node/register", post(register_node))
        .route("/node/status", post(update_node_status))
        .route("/task/submit", post(submit_task))
        .route("/consensus/task-proof", post(submit_task_proof))
        .with_state(state.clone());

    let addr = args.listen;
    info!(%addr, "starting RPC server");
    axum::serve(
        tokio::net::TcpListener::bind(addr).await?,
        app.into_make_service(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

fn spawn_consensus_loop(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        loop {
            interval.tick().await;
            let mut transactions = {
                let mut mempool = state.mempool.lock().await;
                mempool.drain(..).collect::<Vec<_>>()
            };
            let mut ledger = state.ledger.lock().await;
            if let Some(leader) = ledger.select_task_proof_leader() {
                info!(
                    %leader,
                    next_round = ledger.state.network_capacity.consensus_round + 1,
                    "task-proof leader selected"
                );
            }
            let mut task_proof_commits = ledger.collect_task_proof_certificates();
            transactions.append(&mut task_proof_commits);
            match ledger.distribute_block_rewards() {
                Ok(mut rewards) => transactions.append(&mut rewards),
                Err(err) => error!(?err, "failed to compute block rewards"),
            }
            match ledger.commit_block(transactions) {
                Ok(block) => {
                    info!(
                        height = block.height,
                        txs = block.transactions.len(),
                        "block committed"
                    );
                }
                Err(err) => {
                    error!(?err, "failed to commit block");
                }
            }
        }
    });
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install CTRL+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("shutdown signal received");
}

fn new_transaction_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{}", prefix, nanos)
}

async fn get_summary(State(state): State<AppState>) -> Json<NetworkSummary> {
    let ledger = state.ledger.lock().await;
    Json(NetworkSummary::from(&ledger.state))
}

async fn list_blocks(State(state): State<AppState>) -> Json<Vec<Block>> {
    let ledger = state.ledger.lock().await;
    Json(ledger.state.blocks.clone())
}

async fn list_accounts(State(state): State<AppState>) -> Json<Vec<Account>> {
    let ledger = state.ledger.lock().await;
    Json(ledger.state.accounts.values().cloned().collect())
}

async fn list_providers(State(state): State<AppState>) -> Json<Vec<Provider>> {
    let ledger = state.ledger.lock().await;
    Json(ledger.state.providers.values().cloned().collect())
}

async fn list_nodes(State(state): State<AppState>) -> Json<Vec<Node>> {
    let ledger = state.ledger.lock().await;
    Json(ledger.state.nodes.values().cloned().collect())
}

async fn list_tasks(State(state): State<AppState>) -> Json<Vec<Task>> {
    let ledger = state.ledger.lock().await;
    Json(ledger.state.tasks.values().cloned().collect())
}

async fn get_treasury(State(state): State<AppState>) -> Json<Treasury> {
    let ledger = state.ledger.lock().await;
    Json(ledger.state.treasury.clone())
}

#[derive(Debug, Deserialize)]
struct NewTransactionRequest {
    kind: TransactionKind,
}

async fn submit_transaction(
    State(state): State<AppState>,
    Json(payload): Json<NewTransactionRequest>,
) -> impl IntoResponse {
    let tx = Transaction {
        id: new_transaction_id("tx"),
        timestamp: current_timestamp(),
        kind: payload.kind,
    };
    let mut mempool = state.mempool.lock().await;
    mempool.push(tx.clone());
    (StatusCode::ACCEPTED, Json(tx))
}

#[derive(Debug, Deserialize)]
struct ProviderUpsertRequest {
    id: String,
    owner: String,
    endpoint: String,
    region: String,
    gpu_model: String,
    vram_gb: u64,
    bandwidth_gbps: u64,
    cuda: bool,
    rocm: bool,
    reputation: Option<u64>,
    stake_boost: Option<f64>,
    low_tier_discount_bps: Option<u64>,
    pricing: Option<PricingConfig>,
    availability: Option<Vec<AvailabilityWindow>>,
}

async fn upsert_provider(
    State(state): State<AppState>,
    Json(payload): Json<ProviderUpsertRequest>,
) -> impl IntoResponse {
    let mut ledger = state.ledger.lock().await;
    let mut provider = Provider {
        id: payload.id,
        owner: payload.owner,
        endpoint: payload.endpoint,
        region: payload.region,
        gpu_model: payload.gpu_model,
        vram_gb: payload.vram_gb,
        bandwidth_gbps: payload.bandwidth_gbps,
        cuda: payload.cuda,
        rocm: payload.rocm,
        reputation: payload.reputation.unwrap_or_default(),
        stake_boost: payload.stake_boost.unwrap_or(1.0),
        low_tier_discount_bps: payload.low_tier_discount_bps.unwrap_or(0),
        pricing: payload.pricing.unwrap_or_default(),
        availability: payload.availability.unwrap_or_default(),
        model_profiles: Vec::new(),
        last_heartbeat: Some(current_timestamp()),
        throughput_score: 0.0,
    };
    ledger.upsert_provider(provider.clone());
    if let Err(err) = ledger.persist() {
        error!(?err, "failed to persist provider update");
    }
    (StatusCode::OK, Json(provider))
}

#[derive(Debug, Deserialize)]
struct ProviderModelsRequest {
    provider_id: String,
    profiles: Vec<ModelProfile>,
}

async fn set_provider_models(
    State(state): State<AppState>,
    Json(payload): Json<ProviderModelsRequest>,
) -> impl IntoResponse {
    let mut ledger = state.ledger.lock().await;
    match ledger.set_provider_models(&payload.provider_id, payload.profiles.clone()) {
        Ok(()) => {
            if let Err(err) = ledger.persist() {
                error!(?err, "failed to persist provider models");
            }
            StatusCode::OK.into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct ProviderHeartbeatRequest {
    provider_id: String,
}

async fn provider_heartbeat(
    State(state): State<AppState>,
    Json(payload): Json<ProviderHeartbeatRequest>,
) -> impl IntoResponse {
    let mut ledger = state.ledger.lock().await;
    match ledger.heartbeat_provider(&payload.provider_id) {
        Ok(()) => {
            if let Err(err) = ledger.persist() {
                error!(?err, "failed to persist heartbeat");
            }
            StatusCode::OK.into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct RegisterNodeRequest {
    owner: String,
    region: String,
    llm_profile: ModelProfile,
    hardware: NodeHardware,
    metrics: Option<NodeMetrics>,
    fingerprint: Option<String>,
}

async fn register_node(
    State(state): State<AppState>,
    Json(payload): Json<RegisterNodeRequest>,
) -> impl IntoResponse {
    let mut ledger = state.ledger.lock().await;
    let node = Node {
        id: String::new(),
        owner: payload.owner,
        region: payload.region,
        llm_profile: payload.llm_profile,
        online: true,
        reputation: 0,
        registered_at: 0,
        fingerprint: payload.fingerprint.unwrap_or_default(),
        hardware: payload.hardware,
        metrics: payload.metrics.unwrap_or_default(),
    };
    let node = ledger.register_node(node);
    if let Err(err) = ledger.persist() {
        error!(?err, "failed to persist node registration");
    }
    (StatusCode::OK, Json(node))
}

#[derive(Debug, Deserialize)]
struct UpdateNodeStatusRequest {
    node_id: String,
    online: bool,
    metrics: Option<NodeMetrics>,
}

async fn update_node_status(
    State(state): State<AppState>,
    Json(payload): Json<UpdateNodeStatusRequest>,
) -> impl IntoResponse {
    let mut ledger = state.ledger.lock().await;
    match ledger.update_node_status(&payload.node_id, payload.online, payload.metrics) {
        Ok(()) => {
            if let Err(err) = ledger.persist() {
                error!(?err, "failed to persist node status");
            }
            StatusCode::OK.into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct SubmitTaskRequest {
    owner: String,
    content_hash: String,
    total_tokens: u64,
    target_profile: ModelProfile,
    providers: Vec<String>,
    preferred_region: Option<String>,
    #[serde(default)]
    mode: Option<TaskMode>,
    #[serde(default)]
    chat_prompt: Option<String>,
}

async fn submit_task(
    State(state): State<AppState>,
    Json(payload): Json<SubmitTaskRequest>,
) -> impl IntoResponse {
    let SubmitTaskRequest {
        owner,
        content_hash,
        total_tokens,
        target_profile,
        providers,
        preferred_region,
        mode,
        chat_prompt,
    } = payload;
    let mode = mode.unwrap_or(TaskMode::Batch);
    let mut ledger = state.ledger.lock().await;
    let task_id = ledger.submit_task(
        owner,
        content_hash,
        total_tokens,
        target_profile,
        providers,
        preferred_region,
        mode,
        chat_prompt,
    );
    let scheduled = ledger
        .state
        .tasks
        .get(&task_id)
        .map(|task| task.scheduled.clone())
        .unwrap_or_default();
    if let Err(err) = ledger.persist() {
        error!(?err, "failed to persist task submission");
    }
    Json(serde_json::json!({ "task_id": task_id, "scheduled": scheduled }))
}

#[derive(Debug, Deserialize)]
struct TaskProofRequest {
    round: u64,
    task_id: String,
    segment_id: String,
    provider_id: String,
    latency_ms: u64,
    throughput_tok_s: u64,
    tokens_processed: u64,
    region: String,
    signature: String,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    output_digest: Option<String>,
    #[serde(default)]
    chat_response: Option<String>,
}

async fn submit_task_proof(
    State(state): State<AppState>,
    Json(payload): Json<TaskProofRequest>,
) -> impl IntoResponse {
    let TaskProofRequest {
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
    } = payload;
    let tx = Transaction {
        id: new_transaction_id("task-proof"),
        timestamp: current_timestamp(),
        kind: TransactionKind::TaskProofCommit {
            proof: TaskProof {
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
            },
        },
    };
    let mut mempool = state.mempool.lock().await;
    mempool.push(tx.clone());
    (StatusCode::ACCEPTED, Json(tx))
}
