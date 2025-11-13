use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

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

/// Supported execution runtimes for smart contracts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ContractRuntime {
    /// A native GPANG contract interpreted by the ledger itself.
    Native,
    /// A Solana BPF program deployed through GPANG.
    Solana,
}

impl Default for ContractRuntime {
    fn default() -> Self {
        Self::Native
    }
}

impl ContractRuntime {
    /// Canonical string label for the runtime.
    pub fn as_str(&self) -> &'static str {
        match self {
            ContractRuntime::Native => "native",
            ContractRuntime::Solana => "solana",
        }
    }
}

impl FromStr for ContractRuntime {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "native" => Ok(Self::Native),
            "solana" => Ok(Self::Solana),
            other => Err(format!("unsupported contract runtime: {}", other)),
        }
    }
}

/// Distinct operational roles that a node can fulfil in the network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    /// Participates in validation and finality voting.
    Validator,
    /// Executes user workloads and produces task proofs.
    Compute,
    /// Distributes work to compute providers.
    Scheduler,
    /// Builds decentralized task schedules for schedulers to follow.
    Assignment,
}

impl Default for NodeRole {
    fn default() -> Self {
        NodeRole::Compute
    }
}

impl NodeRole {
    /// Readable label used by CLI and explorer surfaces.
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeRole::Validator => "validator",
            NodeRole::Compute => "compute",
            NodeRole::Scheduler => "scheduler",
            NodeRole::Assignment => "assignment",
        }
    }
}

impl FromStr for NodeRole {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "validator" => Ok(NodeRole::Validator),
            "compute" => Ok(NodeRole::Compute),
            "scheduler" => Ok(NodeRole::Scheduler),
            "assignment" => Ok(NodeRole::Assignment),
            other => Err(format!("unsupported node role: {}", other)),
        }
    }
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
    /// Deploys a new smart contract onto the chain.
    DeployContract {
        owner: String,
        #[serde(default)]
        contract_id: Option<String>,
        name: String,
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        metadata: Option<String>,
        #[serde(default)]
        runtime: ContractRuntime,
        #[serde(default)]
        program_id: Option<String>,
        #[serde(default)]
        bytecode_b64: Option<String>,
    },
    /// Executes a smart contract method while recording the payload immutably.
    ExecuteContract {
        contract_id: String,
        caller: String,
        method: String,
        #[serde(default)]
        payload: Value,
    },
    /// Creates a new fungible token definition.
    CreateToken {
        symbol: String,
        name: String,
        decimals: u8,
        initial_supply: u64,
        owner: String,
        #[serde(default)]
        platform: bool,
        #[serde(default)]
        contract_id: Option<String>,
    },
    /// Mints additional supply for a custom token under owner authority.
    MintCustom {
        symbol: String,
        to: String,
        amount: u64,
        authority: String,
    },
    /// Transfers custom token balances between accounts.
    TransferCustom {
        symbol: String,
        from: String,
        to: String,
        amount: u64,
    },
    /// Deposits assets directly into the treasury.
    TreasuryDeposit {
        from: String,
        symbol: String,
        amount: u64,
    },
    /// Withdraws assets from the treasury when authorized.
    TreasuryWithdraw {
        to: String,
        symbol: String,
        amount: u64,
        authority: String,
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
    #[serde(default)]
    pub custom_tokens: HashMap<String, u64>,
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
            custom_tokens: HashMap::new(),
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
    #[serde(default)]
    pub role: NodeRole,
    pub online: bool,
    pub reputation: u64,
    pub registered_at: u64,
    pub fingerprint: String,
    pub nft_token_id: String,
    pub hardware: NodeHardware,
    pub metrics: NodeMetrics,
    pub task_slots_granted: u64,
    pub task_segments_completed: u64,
    pub decentralization_weight: f64,
    pub scheduler_jobs_executed: u64,
    pub assignment_jobs_generated: u64,
    #[serde(default)]
    pub total_rewards: u64,
    #[serde(default)]
    pub last_reward_at: u64,
}

/// NFT asset that backs a node registration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeNft {
    pub token_id: String,
    pub owner: String,
    pub node_id: String,
    pub minted_at: u64,
    pub fingerprint: String,
    pub region: String,
    pub role: NodeRole,
    pub hardware: NodeHardware,
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
    /// Assignment node selected to craft scheduling plans.
    pub assignment_node_id: Option<String>,
    /// Scheduler node chosen to execute scheduling plans.
    pub scheduler_node_id: Option<String>,
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
    #[serde(default)]
    pub custom_tokens: HashMap<String, u64>,
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
            custom_tokens: HashMap::new(),
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

/// Metadata describing a deployed smart contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartContract {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub code: String,
    pub code_hash: String,
    pub metadata: Option<String>,
    pub deployed_at: u64,
    #[serde(default)]
    pub runtime: ContractRuntime,
    #[serde(default)]
    pub program_id: Option<String>,
    #[serde(default)]
    pub bytecode_b64: Option<String>,
}

/// Immutable execution log emitted whenever a contract runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractEvent {
    pub id: String,
    pub contract_id: String,
    pub caller: String,
    pub method: String,
    pub payload: Value,
    pub payload_hash: String,
    pub timestamp: u64,
}

/// Definition of a fungible token deployed by the network or users.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenDefinition {
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub total_supply: u128,
    pub owner: String,
    pub platform: bool,
    pub created_at: u64,
    pub contract_id: Option<String>,
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
    #[serde(default)]
    pub node_nfts: HashMap<String, NodeNft>,
    pub tasks: HashMap<String, Task>,
    pub blocks: Vec<Block>,
    pub treasury: Treasury,
    pub next_task_id: u64,
    #[serde(default = "default_next_node_id")]
    pub next_node_id: u64,
    #[serde(default = "default_next_node_nft_id")]
    pub next_node_nft_id: u64,
    pub pending_task_proofs: Vec<TaskProof>,
    pub network_capacity: NetworkCapacity,
    #[serde(default)]
    pub contracts: HashMap<String, SmartContract>,
    #[serde(default)]
    pub contract_events: Vec<ContractEvent>,
    #[serde(default)]
    pub token_definitions: HashMap<String, TokenDefinition>,
    #[serde(default = "default_next_contract_id")]
    pub next_contract_id: u64,
    #[serde(default)]
    pub platform_tokens: HashSet<String>,
}

impl Default for LedgerState {
    fn default() -> Self {
        Self {
            accounts: HashMap::new(),
            providers: HashMap::new(),
            nodes: HashMap::new(),
            node_nfts: HashMap::new(),
            tasks: HashMap::new(),
            blocks: Vec::new(),
            treasury: Treasury::default(),
            next_task_id: 1,
            next_node_id: 1,
            next_node_nft_id: 1,
            pending_task_proofs: Vec::new(),
            network_capacity: NetworkCapacity::default(),
            contracts: HashMap::new(),
            contract_events: Vec::new(),
            token_definitions: HashMap::new(),
            next_contract_id: 1,
            platform_tokens: HashSet::new(),
        }
    }
}

fn default_next_node_id() -> u64 {
    1
}

fn default_next_node_nft_id() -> u64 {
    1
}

fn default_next_contract_id() -> u64 {
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
        foundation.stake_balance = 200_000_000_000;
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

        // Seed a reference contract so explorers can highlight smart contract support.
        let demo_contract = SmartContract {
            id: "contract-1".to_string(),
            owner: "foundation".to_string(),
            name: "foundation-airdrop".to_string(),
            code: "fn distribute() { /* demo */ }".to_string(),
            code_hash: "foundation-airdrop-demo".to_string(),
            metadata: Some("distributes welcome rewards".to_string()),
            deployed_at: timestamp,
            runtime: ContractRuntime::Native,
            program_id: None,
            bytecode_b64: None,
        };
        state.next_contract_id = 2;
        state
            .contracts
            .insert(demo_contract.id.clone(), demo_contract.clone());

        let platform_token = TokenDefinition {
            symbol: "GPANGP".to_string(),
            name: "GPANG Platform".to_string(),
            decimals: 9,
            total_supply: 10_000_000_000_000,
            owner: "foundation".to_string(),
            platform: true,
            created_at: timestamp,
            contract_id: Some("contract-1".to_string()),
        };
        state
            .treasury
            .custom_tokens
            .insert(platform_token.symbol.clone(), 10_000_000_000_000);
        state.platform_tokens.insert(platform_token.symbol.clone());
        state
            .token_definitions
            .insert(platform_token.symbol.clone(), platform_token.clone());

        let builder_token = TokenDefinition {
            symbol: "BLDR".to_string(),
            name: "Builder Reward".to_string(),
            decimals: 6,
            total_supply: 1_000_000_000,
            owner: "builder".to_string(),
            platform: false,
            created_at: timestamp,
            contract_id: None,
        };
        if let Some(account) = state.accounts.get_mut("builder") {
            account.custom_tokens.insert(
                builder_token.symbol.clone(),
                builder_token.total_supply as u64,
            );
        }
        state
            .token_definitions
            .insert(builder_token.symbol.clone(), builder_token.clone());

        state.contract_events.push(ContractEvent {
            id: "event-1".to_string(),
            contract_id: "contract-1".to_string(),
            caller: "foundation".to_string(),
            method: "distribute".to_string(),
            payload: Value::String("genesis invocation".to_string()),
            payload_hash: "foundation-airdrop-event".to_string(),
            timestamp,
        });

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
            role: NodeRole::Validator,
            online: true,
            reputation: 990,
            registered_at: timestamp,
            fingerprint: "0xalpha".to_string(),
            nft_token_id: "node-nft-1".to_string(),
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
            scheduler_jobs_executed: 0,
            assignment_jobs_generated: 0,
            total_rewards: 0,
            last_reward_at: 0,
        };
        let node_beta = Node {
            id: "node-beta".to_string(),
            owner: "builder".to_string(),
            region: "NA-USA".to_string(),
            llm_profile: profile_usa,
            role: NodeRole::Compute,
            online: true,
            reputation: 905,
            registered_at: timestamp,
            fingerprint: "0xbeta".to_string(),
            nft_token_id: "node-nft-2".to_string(),
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
            scheduler_jobs_executed: 0,
            assignment_jobs_generated: 0,
            total_rewards: 0,
            last_reward_at: 0,
        };
        let node_scheduler = Node {
            id: "node-scheduler".to_string(),
            owner: "builder".to_string(),
            region: "EU-DE".to_string(),
            llm_profile: profile_flagship.clone(),
            role: NodeRole::Scheduler,
            online: true,
            reputation: 850,
            registered_at: timestamp,
            fingerprint: "0xscheduler".to_string(),
            nft_token_id: "node-nft-3".to_string(),
            hardware: NodeHardware {
                cpu_model: "AMD EPYC 7713".to_string(),
                cpu_cores: 64,
                cpu_threads: 128,
                memory_total_mb: 1_572_864,
                gpu_vendor: "NVIDIA".to_string(),
                gpu_model: "A100".to_string(),
                gpu_vram_mb: 40_000,
                os: "Linux".to_string(),
            },
            metrics: NodeMetrics {
                timestamp,
                cpu_usage_pct: 15.0,
                memory_usage_pct: 35.0,
                gpu_usage_pct: 10.0,
                machine_load_one: 1.2,
                machine_load_five: 1.0,
                machine_load_fifteen: 0.8,
                disk_usage_pct: 45.0,
                disk_read_mbps: 50.0,
                disk_write_mbps: 42.0,
                network_rx_mbps: 380.0,
                network_tx_mbps: 365.0,
                gpu_memory_used_mb: 12_000,
                gpu_memory_total_mb: 40_000,
            },
            task_slots_granted: 0,
            task_segments_completed: 0,
            decentralization_weight: 1.0,
            scheduler_jobs_executed: 6,
            assignment_jobs_generated: 0,
            total_rewards: 0,
            last_reward_at: 0,
        };
        let node_assignment = Node {
            id: "node-assignment".to_string(),
            owner: "foundation".to_string(),
            region: "AP-SEA".to_string(),
            llm_profile: profile_flagship,
            role: NodeRole::Assignment,
            online: true,
            reputation: 920,
            registered_at: timestamp,
            fingerprint: "0xassignment".to_string(),
            nft_token_id: "node-nft-4".to_string(),
            hardware: NodeHardware {
                cpu_model: "Apple M2 Ultra".to_string(),
                cpu_cores: 24,
                cpu_threads: 24,
                memory_total_mb: 262_144,
                gpu_vendor: "Apple".to_string(),
                gpu_model: "M2 Ultra".to_string(),
                gpu_vram_mb: 64_000,
                os: "macOS".to_string(),
            },
            metrics: NodeMetrics {
                timestamp,
                cpu_usage_pct: 18.0,
                memory_usage_pct: 42.0,
                gpu_usage_pct: 15.0,
                machine_load_one: 1.1,
                machine_load_five: 0.9,
                machine_load_fifteen: 0.7,
                disk_usage_pct: 30.0,
                disk_read_mbps: 38.0,
                disk_write_mbps: 25.0,
                network_rx_mbps: 240.0,
                network_tx_mbps: 255.0,
                gpu_memory_used_mb: 9_000,
                gpu_memory_total_mb: 64_000,
            },
            task_slots_granted: 0,
            task_segments_completed: 0,
            decentralization_weight: 1.0,
            scheduler_jobs_executed: 0,
            assignment_jobs_generated: 5,
            total_rewards: 0,
            last_reward_at: 0,
        };

        let nft_alpha = NodeNft {
            token_id: node_alpha.nft_token_id.clone(),
            owner: node_alpha.owner.clone(),
            node_id: node_alpha.id.clone(),
            minted_at: node_alpha.registered_at,
            fingerprint: node_alpha.fingerprint.clone(),
            region: node_alpha.region.clone(),
            role: node_alpha.role,
            hardware: node_alpha.hardware.clone(),
        };
        let nft_beta = NodeNft {
            token_id: node_beta.nft_token_id.clone(),
            owner: node_beta.owner.clone(),
            node_id: node_beta.id.clone(),
            minted_at: node_beta.registered_at,
            fingerprint: node_beta.fingerprint.clone(),
            region: node_beta.region.clone(),
            role: node_beta.role,
            hardware: node_beta.hardware.clone(),
        };
        let nft_scheduler = NodeNft {
            token_id: node_scheduler.nft_token_id.clone(),
            owner: node_scheduler.owner.clone(),
            node_id: node_scheduler.id.clone(),
            minted_at: node_scheduler.registered_at,
            fingerprint: node_scheduler.fingerprint.clone(),
            region: node_scheduler.region.clone(),
            role: node_scheduler.role,
            hardware: node_scheduler.hardware.clone(),
        };
        let nft_assignment = NodeNft {
            token_id: node_assignment.nft_token_id.clone(),
            owner: node_assignment.owner.clone(),
            node_id: node_assignment.id.clone(),
            minted_at: node_assignment.registered_at,
            fingerprint: node_assignment.fingerprint.clone(),
            region: node_assignment.region.clone(),
            role: node_assignment.role,
            hardware: node_assignment.hardware.clone(),
        };

        state.nodes.insert(node_alpha.id.clone(), node_alpha);
        state.nodes.insert(node_beta.id.clone(), node_beta);
        state
            .nodes
            .insert(node_scheduler.id.clone(), node_scheduler);
        state
            .nodes
            .insert(node_assignment.id.clone(), node_assignment);
        state
            .node_nfts
            .insert(nft_alpha.token_id.clone(), nft_alpha);
        state.node_nfts.insert(nft_beta.token_id.clone(), nft_beta);
        state
            .node_nfts
            .insert(nft_scheduler.token_id.clone(), nft_scheduler);
        state
            .node_nfts
            .insert(nft_assignment.token_id.clone(), nft_assignment);

        state.next_node_id = 5;
        state.next_node_nft_id = 5;

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
