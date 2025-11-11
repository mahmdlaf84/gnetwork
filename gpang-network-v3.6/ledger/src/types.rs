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
    /// Records the result of a task-proof commitment used for consensus.
    TaskProofCommit {
        proof: TaskProof,
    },
}

/// Transaction wrapper storing metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub timestamp: u64,
    pub kind: TransactionKind,
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

/// Registered node metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Node {
    pub id: String,
    pub owner: String,
    pub gpu_model: String,
    pub region: String,
    pub llm_profile: ModelProfile,
    pub online: bool,
    pub reputation: u64,
    pub registered_at: u64,
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

/// Task lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
        }
    }
}

impl Treasury {
    pub fn with_balances(aia: u64, work: u64, stor: u64) -> Self {
        Self {
            aia_balance: aia,
            work_balance: work,
            stor_balance: stor,
            ..Self::default()
        }
    }
}

/// Proof emitted by providers to participate in task-proof consensus.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

/// Aggregate metrics for measuring network scalability targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCapacity {
    pub consensus_round: u64,
    pub tokens_processed: u128,
    pub target_tokens_per_sec: u64,
    pub max_parallel_nodes: u64,
    pub peak_observed_tokens_per_sec: u64,
}

impl Default for NetworkCapacity {
    fn default() -> Self {
        Self {
            consensus_round: 0,
            tokens_processed: 0,
            target_tokens_per_sec: 10_000_000,
            max_parallel_nodes: 100_000_000,
            peak_observed_tokens_per_sec: 0,
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
            pending_task_proofs: Vec::new(),
            network_capacity: NetworkCapacity::default(),
        }
    }
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
            gpu_model: "NVIDIA H100".to_string(),
            region: "AP-SEA".to_string(),
            llm_profile: profile_flagship.clone(),
            online: true,
            reputation: 990,
            registered_at: timestamp,
        };
        let node_beta = Node {
            id: "node-beta".to_string(),
            owner: "builder".to_string(),
            gpu_model: "RTX 6000 Ada".to_string(),
            region: "NA-USA".to_string(),
            llm_profile: profile_usa,
            online: true,
            reputation: 905,
            registered_at: timestamp,
        };

        state.nodes.insert(node_alpha.id.clone(), node_alpha);
        state.nodes.insert(node_beta.id.clone(), node_beta);

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
