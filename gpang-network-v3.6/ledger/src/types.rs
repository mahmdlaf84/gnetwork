use chrono::{DateTime, Utc};
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
}

/// Transaction wrapper storing metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub timestamp: DateTime<Utc>,
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
    pub last_heartbeat: Option<DateTime<Utc>>,
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
    pub registered_at: DateTime<Utc>,
}

/// Captures the outcome of a FlashRace segment.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskSegment {
    pub segment_id: String,
    pub provider_id: String,
    pub tokens: u64,
    pub latency_ms: u64,
    pub reward: u64,
    pub submitted_at: DateTime<Utc>,
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
    pub created_at: DateTime<Utc>,
    pub completed: bool,
    pub winning_provider: Option<String>,
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

/// A persisted block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub height: u64,
    pub timestamp: DateTime<Utc>,
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
        }
    }
}
