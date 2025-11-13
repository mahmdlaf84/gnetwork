use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Enumeration of all supported token denominations in the GPANG Network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// The main network currency used for staking and payments.
    AIA,
    /// Reward token for compute contributions.
    WORK,
    /// Reward token for storage contributions.
    STOR,
}

/// A high level description of a transaction payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransactionKind {
    Mint {
        to: String,
        token: TokenKind,
        amount: u64,
        reason: Option<String>,
    },
    Burn {
        from: String,
        token: TokenKind,
        amount: u64,
        reason: Option<String>,
    },
    Transfer {
        from: String,
        to: String,
        token: TokenKind,
        amount: u64,
        memo: Option<String>,
    },
    Stake {
        owner: String,
        amount: u64,
    },
    Airdrop {
        to: String,
        token: TokenKind,
        amount: u64,
        campaign: Option<String>,
    },
    Payout {
        to: String,
        token: TokenKind,
        amount: u64,
        task_id: String,
    },
    SegmentReceipt {
        task_id: String,
        segment_id: String,
        provider_id: String,
        tokens: u64,
        latency_ms: u64,
        reward: u64,
    },
    /// Records the aggregated output of a multi-node chat task.
    ChatResult {
        task_id: String,
        aggregate: String,
        contributors: Vec<String>,
    },
    /// Records the result of a task-proof commitment used for consensus.
    TaskProofCommit {
        proof: TaskProof,
    },
}

/// Transaction wrapper storing metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Transaction {
    pub id: String,
    pub timestamp: u64,
    pub kind: TransactionKind,
    pub gas_payer: Option<String>,
    pub gas_limit: u64,
    pub gas_price: u64,
    pub gas_used: u64,
    pub zero_proof: Option<ZeroProof>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ZeroProof {
    pub scheme: String,
    pub statement: String,
    pub digest: String,
}

impl ZeroProof {
    pub fn new(scheme: impl Into<String>, statement: impl Into<String>) -> Self {
        let scheme = scheme.into();
        let statement = statement.into();
        let digest = Self::hash(&scheme, &statement);
        Self {
            scheme,
            statement,
            digest,
        }
    }

    fn hash(scheme: &str, statement: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        scheme.hash(&mut hasher);
        statement.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    pub fn verify(&self) -> bool {
        !self.scheme.is_empty()
            && !self.statement.is_empty()
            && !self.digest.is_empty()
            && Self::hash(&self.scheme, &self.statement) == self.digest
    }
}

/// Account level information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub aia_balance: u64,
    pub work_balance: u64,
    pub stor_balance: u64,
    pub stake_balance: u64,
    pub total_earnings: u64,
    pub total_costs: u64,
}

impl Account {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            aia_balance: 0,
            work_balance: 0,
            stor_balance: 0,
            stake_balance: 0,
            total_earnings: 0,
            total_costs: 0,
        }
    }
}

/// GPU pricing envelope.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PricingConfig {
    pub base_per_token: u64,
    pub min_per_token: u64,
    pub max_per_token: u64,
    pub current_per_token: u64,
}

/// Declares when a provider can accept work.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AvailabilityWindow {
    pub day_of_week: String,
    pub start_hour: u8,
    pub end_hour: u8,
}

/// Describes a concrete model profile served by a provider or node.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelProfile {
    pub model_id: String,
    pub quant: String,
    pub max_ctx: u64,
    pub vram_req_gb: u64,
    pub throughput_tok_s: u64,
}

/// Registered GPU provider metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Provider {
    pub id: String,
    pub owner: String,
    pub endpoint: String,
    pub region: String,
    pub gpu_model: String,
    pub vram_gb: u64,
    pub bandwidth_gbps: u64,
    pub cuda: bool,
    pub rocm: bool,
    pub reputation: u64,
    pub stake_boost: f64,
    pub low_tier_discount_bps: u64,
    pub pricing: PricingConfig,
    pub availability: Vec<AvailabilityWindow>,
    pub model_profiles: Vec<ModelProfile>,
    pub last_heartbeat: Option<u64>,
    /// Cached normalized throughput score used for scheduling.
    pub throughput_score: f64,
}

/// Static hardware capabilities reported by a node when registering.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeHardware {
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub memory_total_mb: u64,
    pub gpu_vendor: String,
    pub gpu_model: String,
    pub gpu_vram_mb: u64,
    pub os: String,
}

/// Most recent runtime metrics reported by a node.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeMetrics {
    pub timestamp: u64,
    pub cpu_usage_pct: f32,
    pub memory_usage_pct: f32,
    pub gpu_usage_pct: f32,
    pub machine_load_one: f32,
    pub machine_load_five: f32,
    pub machine_load_fifteen: f32,
    pub disk_usage_pct: f32,
    pub disk_read_mbps: f32,
    pub disk_write_mbps: f32,
    pub network_rx_mbps: f32,
    pub network_tx_mbps: f32,
    pub gpu_memory_used_mb: u64,
    pub gpu_memory_total_mb: u64,
}

/// Registered node metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Node {
    pub id: String,
    pub owner: String,
    pub region: String,
    pub llm_profile: ModelProfile,
    pub online: bool,
    pub reputation: u64,
    pub registered_at: u64,
    pub fingerprint: String,
    pub hardware: NodeHardware,
    pub metrics: NodeMetrics,
    pub task_slots_granted: u64,
    pub task_segments_completed: u64,
    pub decentralization_weight: f64,
}

/// Captures the outcome of a FlashRace segment.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskSegment {
    pub segment_id: String,
    pub provider_id: String,
    pub tokens: u64,
    pub latency_ms: u64,
    pub reward: u64,
    pub submitted_at: u64,
}

/// Supported execution modes for tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskMode {
    /// Traditional throughput-oriented inference request.
    Batch,
    /// Conversational request that benefits from multi-node aggregation.
    Chat,
}

impl Default for TaskMode {
    fn default() -> Self {
        TaskMode::Batch
    }
}

/// A node's chat response captured for aggregation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatResponse {
    pub provider_id: String,
    pub node_id: Option<String>,
    pub segment_id: String,
    pub latency_ms: u64,
    pub tokens: u64,
    pub response_fragment: String,
    pub submitted_at: u64,
}

/// Task lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Task {
    pub id: String,
    pub owner: String,
    pub content_hash: String,
    pub total_tokens: u64,
    pub target_profile: ModelProfile,
    pub requested_providers: Vec<String>,
    pub segments: Vec<TaskSegment>,
    pub created_at: u64,
    pub completed: bool,
    pub winning_provider: Option<String>,
    pub preferred_region: Option<String>,
    /// Providers recommended by the adaptive scheduler.
    pub scheduled: Vec<ScheduledProvider>,
    /// Execution mode requested by the submitter.
    pub mode: TaskMode,
    /// Optional chat prompt when `mode` is chat.
    pub chat_prompt: Option<String>,
    /// Individual chat responses collected from nodes.
    pub chat_responses: Vec<ChatResponse>,
    /// Aggregated and optimized chat output.
    pub chat_aggregate: Option<String>,
}

/// Scheduler output for a provider.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduledProvider {
    pub provider_id: String,
    pub score: f64,
    pub expected_latency_ms: u64,
    pub max_tokens_per_sec: u64,
}

/// Treasury balances and parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Treasury {
    pub aia_balance: u64,
    pub work_balance: u64,
    pub stor_balance: u64,
    pub fee_bps: u64,
    pub reward_bps: u64,
    pub low_tier_discount_bps: u64,
    pub gas_collected: u64,
}

impl Default for Treasury {
    fn default() -> Self {
        Self {
            aia_balance: 0,
            work_balance: 0,
            stor_balance: 0,
            fee_bps: 500,
            reward_bps: 200,
            low_tier_discount_bps: 1000,
            gas_collected: 0,
        }
    }
}

impl Treasury {
    pub fn with_balances(aia: u64, work: u64, stor: u64) -> Self {
        Self {
            aia_balance: aia,
            work_balance: work,
            stor_balance: stor,
            gas_collected: 0,
            ..Self::default()
        }
    }
}

/// Proof emitted by providers to participate in task-proof consensus.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TaskProof {
    pub round: u64,
    pub task_id: String,
    pub segment_id: String,
    pub provider_id: String,
    pub latency_ms: u64,
    pub throughput_tok_s: u64,
    pub tokens_processed: u64,
    pub region: String,
    pub signature: String,
    pub node_id: Option<String>,
    pub output_digest: Option<String>,
    pub chat_response: Option<String>,
}

/// Aggregate metrics for measuring network scalability targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCapacity {
    pub consensus_round: u64,
    pub tokens_processed: u128,
    pub target_tokens_per_sec: u64,
    pub max_parallel_nodes: u64,
    pub peak_observed_tokens_per_sec: u64,
    pub total_task_slots: u128,
    pub average_tasks_per_node: f64,
}

impl Default for NetworkCapacity {
    fn default() -> Self {
        Self {
            consensus_round: 0,
            tokens_processed: 0,
            target_tokens_per_sec: 10_000_000,
            max_parallel_nodes: 100_000_000,
            peak_observed_tokens_per_sec: 0,
            total_task_slots: 0,
            average_tasks_per_node: 0.0,
        }
    }
}

/// A persisted block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub height: u64,
    pub timestamp: u64,
    pub transactions: Vec<Transaction>,
}

/// Persistent on-chain state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerState {
    pub accounts: HashMap<String, Account>,
    pub providers: HashMap<String, Provider>,
    pub nodes: HashMap<String, Node>,
    pub tasks: HashMap<String, Task>,
    pub blocks: Vec<Block>,
    pub treasury: Treasury,
    pub next_task_id: u64,
    #[serde(default = "default_next_node_id")]
    pub next_node_id: u64,
    pub pending_task_proofs: Vec<TaskProof>,
    pub network_capacity: NetworkCapacity,
}

impl Default for LedgerState {
    fn default() -> Self {
        Self {
            accounts: HashMap::new(),
            providers: HashMap::new(),
            nodes: HashMap::new(),
            tasks: HashMap::new(),
            blocks: Vec::new(),
            treasury: Treasury::default(),
            next_task_id: 1,
            next_node_id: 1,
            pending_task_proofs: Vec::new(),
            network_capacity: NetworkCapacity::default(),
        }
    }
}

fn default_next_node_id() -> u64 {
    1
}

impl LedgerState {
    /// Constructs a deterministic testnet genesis snapshot used for local development.
    pub fn testnet_genesis(timestamp: u64) -> Self {
        let mut state = Self::default();

        let mut foundation = Account::new("foundation");
        foundation.aia_balance = 1_000_000_000_000;
        foundation.work_balance = 500_000_000_000;
        foundation.stor_balance = 200_000_000_000;
        foundation.total_earnings = foundation.aia_balance;
        state.accounts.insert(foundation.id.clone(), foundation);

        let mut builder = Account::new("builder");
        builder.aia_balance = 250_000_000_000;
        builder.work_balance = 100_000_000_000;
        builder.total_earnings = builder.aia_balance;
        state.accounts.insert(builder.id.clone(), builder);

        let mut validator = Account::new("validator-1");
        validator.aia_balance = 150_000_000_000;
        validator.stake_balance = 100_000_000_000;
        validator.total_earnings = validator.aia_balance;
        state.accounts.insert(validator.id.clone(), validator);

        let profile_flagship = ModelProfile {
            model_id: "qwen2.5-7b".to_string(),
            quant: "int4".to_string(),
            max_ctx: 8192,
            vram_req_gb: 12,
            throughput_tok_s: 320_000,
        };
        let profile_usa = ModelProfile {
            model_id: "qwen2.5-14b".to_string(),
            quant: "int4".to_string(),
            max_ctx: 16_384,
            vram_req_gb: 24,
            throughput_tok_s: 280_000,
        };

        let availability = vec![AvailabilityWindow {
            day_of_week: "Mon-Sun".to_string(),
            start_hour: 0,
            end_hour: 24,
        }];

        let provider_flagship = Provider {
            id: "prov-asia".to_string(),
            owner: "foundation".to_string(),
            endpoint: "http://127.0.0.1:8080".to_string(),
            region: "AP-SEA".to_string(),
            gpu_model: "NVIDIA H100".to_string(),
            vram_gb: 80,
            bandwidth_gbps: 400,
            cuda: true,
            rocm: false,
            reputation: 980,
            stake_boost: 1.25,
            low_tier_discount_bps: 0,
            pricing: PricingConfig {
                base_per_token: 10,
                min_per_token: 6,
                max_per_token: 18,
                current_per_token: 10,
            },
            availability: availability.clone(),
            model_profiles: vec![profile_flagship.clone()],
            last_heartbeat: Some(timestamp),
            throughput_score: 320_000.0,
        };

        let provider_us = Provider {
            id: "prov-na".to_string(),
            owner: "builder".to_string(),
            endpoint: "http://127.0.0.1:8090".to_string(),
            region: "NA-USA".to_string(),
            gpu_model: "RTX 6000 Ada".to_string(),
            vram_gb: 48,
            bandwidth_gbps: 200,
            cuda: true,
            rocm: false,
            reputation: 870,
            stake_boost: 1.15,
            low_tier_discount_bps: 250,
            pricing: PricingConfig {
                base_per_token: 7,
                min_per_token: 5,
                max_per_token: 15,
                current_per_token: 7,
            },
            availability: availability.clone(),
            model_profiles: vec![profile_flagship.clone(), profile_usa.clone()],
            last_heartbeat: Some(timestamp),
            throughput_score: 280_000.0,
        };

        state
            .providers
            .insert(provider_flagship.id.clone(), provider_flagship);
        state.providers.insert(provider_us.id.clone(), provider_us);

        let node_alpha = Node {
            id: "node-alpha".to_string(),
            owner: "foundation".to_string(),
            region: "AP-SEA".to_string(),
            llm_profile: profile_flagship.clone(),
            online: true,
            reputation: 990,
            registered_at: timestamp,
            fingerprint: "0xalpha".to_string(),
            hardware: NodeHardware {
                cpu_model: "AMD EPYC 9654".to_string(),
                cpu_cores: 96,
                cpu_threads: 192,
                memory_total_mb: 2_097_152,
                gpu_vendor: "NVIDIA".to_string(),
                gpu_model: "H100".to_string(),
                gpu_vram_mb: 80_000,
                os: "Linux".to_string(),
            },
            metrics: NodeMetrics {
                timestamp,
                cpu_usage_pct: 12.5,
                memory_usage_pct: 40.0,
                gpu_usage_pct: 18.0,
                machine_load_one: 1.5,
                machine_load_five: 1.1,
                machine_load_fifteen: 0.9,
                disk_usage_pct: 55.0,
                disk_read_mbps: 120.0,
                disk_write_mbps: 90.0,
                network_rx_mbps: 800.0,
                network_tx_mbps: 760.0,
                gpu_memory_used_mb: 32_000,
                gpu_memory_total_mb: 80_000,
            },
            task_slots_granted: 0,
            task_segments_completed: 0,
            decentralization_weight: 1.0,
        };
        let node_beta = Node {
            id: "node-beta".to_string(),
            owner: "builder".to_string(),
            region: "NA-USA".to_string(),
            llm_profile: profile_usa,
            online: true,
            reputation: 905,
            registered_at: timestamp,
            fingerprint: "0xbeta".to_string(),
            hardware: NodeHardware {
                cpu_model: "Intel Xeon Platinum 8490H".to_string(),
                cpu_cores: 60,
                cpu_threads: 120,
                memory_total_mb: 1_048_576,
                gpu_vendor: "NVIDIA".to_string(),
                gpu_model: "RTX 6000 Ada".to_string(),
                gpu_vram_mb: 48_000,
                os: "Linux".to_string(),
            },
            metrics: NodeMetrics {
                timestamp,
                cpu_usage_pct: 22.0,
                memory_usage_pct: 55.0,
                gpu_usage_pct: 30.0,
                machine_load_one: 2.0,
                machine_load_five: 1.4,
                machine_load_fifteen: 1.1,
                disk_usage_pct: 61.0,
                disk_read_mbps: 80.0,
                disk_write_mbps: 72.0,
                network_rx_mbps: 420.0,
                network_tx_mbps: 405.0,
                gpu_memory_used_mb: 20_000,
                gpu_memory_total_mb: 48_000,
            },
            task_slots_granted: 0,
            task_segments_completed: 0,
            decentralization_weight: 1.0,
        };

        state.nodes.insert(node_alpha.id.clone(), node_alpha);
        state.nodes.insert(node_beta.id.clone(), node_beta);

        state.next_node_id = 3;

        state.treasury = Treasury::with_balances(900_000_000_000, 400_000_000_000, 150_000_000_000);
        state.network_capacity.target_tokens_per_sec = 10_000_000;
        state.network_capacity.max_parallel_nodes = 100_000_000;
        state.network_capacity.peak_observed_tokens_per_sec = 6_400_000;

        state.blocks.push(Block {
            height: 0,
            timestamp,
            transactions: Vec::new(),
        });

        state
    }
}
