//! GPANG Network ledger module.
//!
//! The ledger owns all on-chain state including accounts, GPU providers, nodes,
//! and task execution records. State is persisted as JSON on disk so that the
//! entire blockchain can operate without an external database. All mutating
//! operations funnel through [`Ledger::apply_transaction`] to provide a single
//! entrypoint for token and reward accounting.

use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tracing::info;

pub mod types;

use types::{
    Account, Block, ChatResponse, LedgerState, ModelProfile, Node, NodeHardware, NodeMetrics,
    Provider, ScheduledProvider, Task, TaskMode, TaskProof, TaskSegment, TokenKind, Transaction,
    TransactionKind,
};

/// Returns the current Unix timestamp in milliseconds.
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Default persistence file name.
pub const DEFAULT_LEDGER_FILE: &str = "ledger_v35.json";

/// Errors that can be produced by the ledger.
#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("account {0} has insufficient balance")]
    InsufficientBalance(String),
    #[error("account {0} not found")]
    AccountNotFound(String),
    #[error("task {0} not found")]
    TaskNotFound(String),
    #[error("provider {0} not found")]
    ProviderNotFound(String),
    #[error("node {0} not found")]
    NodeNotFound(String),
    #[error("transaction validation error: {0}")]
    InvalidTransaction(String),
}

/// Top-level ledger handle that keeps the path to the persistence file.
#[derive(Debug, Clone)]
pub struct Ledger {
    path: PathBuf,
    pub state: LedgerState,
}

impl Ledger {
    /// Opens or creates a ledger at the provided path.
    pub fn load_or_initialize(path: impl AsRef<Path>) -> Result<Self, LedgerError> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            let data = fs::read_to_string(&path)?;
            let state: LedgerState = serde_json::from_str(&data)?;
            Ok(Self { path, state })
        } else {
            let state = LedgerState::default();
            let ledger = Self {
                path: path.clone(),
                state,
            };
            ledger.persist()?;
            Ok(ledger)
        }
    }

    /// Initializes a testnet ledger snapshot, overwriting any prior file at the path.
    pub fn initialize_testnet(path: impl AsRef<Path>) -> Result<Self, LedgerError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        if path.exists() {
            fs::remove_file(&path)?;
        }

        let state = LedgerState::testnet_genesis(current_timestamp());
        let ledger = Self {
            path: path.clone(),
            state,
        };
        ledger.persist()?;
        Ok(ledger)
    }

    /// Persists the current state to disk.
    pub fn persist(&self) -> Result<(), LedgerError> {
        let data = serde_json::to_string_pretty(&self.state)?;
        fs::write(&self.path, data)?;
        Ok(())
    }

    /// Fetches or creates an account.
    fn get_or_create_account(&mut self, id: &str) -> &mut Account {
        self.state
            .accounts
            .entry(id.to_string())
            .or_insert_with(|| Account::new(id))
    }

    fn get_account(&mut self, id: &str) -> Result<&mut Account, LedgerError> {
        self.state
            .accounts
            .get_mut(id)
            .ok_or_else(|| LedgerError::AccountNotFound(id.to_string()))
    }

    fn adjust_balance(
        account: &mut Account,
        token: &TokenKind,
        delta: i128,
    ) -> Result<(), LedgerError> {
        let (balance, name) = match token {
            TokenKind::AIA => (&mut account.aia_balance, "AIA"),
            TokenKind::WORK => (&mut account.work_balance, "WORK"),
            TokenKind::STOR => (&mut account.stor_balance, "STOR"),
        };
        let new_value = *balance as i128 + delta;
        if new_value < 0 {
            return Err(LedgerError::InsufficientBalance(format!(
                "{} for account {}",
                name, account.id
            )));
        }
        *balance = new_value as u64;
        Ok(())
    }

    fn provider_peak_throughput(provider: &Provider) -> f64 {
        provider
            .model_profiles
            .iter()
            .map(|profile| profile.throughput_tok_s as f64)
            .fold(0.0, |acc, next| acc.max(next))
            .max(provider.throughput_score)
            .max((provider.bandwidth_gbps as f64) * 1_000.0)
    }

    fn recalculate_average_tasks(&mut self) {
        let active = self.state.nodes.values().filter(|node| node.online).count() as f64;
        let denom = if active > 0.0 { active } else { 1.0 };
        self.state.network_capacity.average_tasks_per_node =
            self.state.network_capacity.total_task_slots as f64 / denom;
    }

    fn decentralization_weight_for(
        task_slots: u64,
        segments: u64,
        average: f64,
        throughput: u64,
    ) -> f64 {
        let avg = average.max(1.0);
        let load_ratio = task_slots as f64 / avg;
        let fairness = if load_ratio >= 1.0 {
            1.0 / (1.0 + (load_ratio - 1.0))
        } else {
            1.0 + (1.0 - load_ratio) * 0.3
        }
        .clamp(0.5, 2.0);
        let completion_ratio = if task_slots == 0 {
            1.0
        } else {
            (segments as f64 / task_slots as f64).clamp(0.25, 1.5)
        };
        let throughput_factor = ((throughput as f64 / 100_000.0).max(0.1))
            .sqrt()
            .clamp(0.5, 2.0);
        fairness * completion_ratio * throughput_factor
    }

    fn apply_task_share_for_node(&mut self, node_id: &str) {
        {
            let Some(node) = self.state.nodes.get_mut(node_id) else {
                return;
            };
            node.task_slots_granted = node.task_slots_granted.saturating_add(1);
            node.task_segments_completed = node.task_segments_completed.saturating_add(1);
        }
        self.state.network_capacity.total_task_slots = self
            .state
            .network_capacity
            .total_task_slots
            .saturating_add(1);
        self.recalculate_average_tasks();
        if let Some(node) = self.state.nodes.get_mut(node_id) {
            node.decentralization_weight = Self::decentralization_weight_for(
                node.task_slots_granted,
                node.task_segments_completed,
                self.state.network_capacity.average_tasks_per_node,
                node.llm_profile.throughput_tok_s,
            );
        }
    }

    fn consensus_score_for_node(&self, node: &Node, owner_stake: f64) -> f64 {
        let reputation_factor = 1.0 + (node.reputation as f64 / 1_000.0);
        let throughput_factor =
            ((node.llm_profile.throughput_tok_s as f64 / 1_000.0).max(1.0)).sqrt();
        let fairness = if node.decentralization_weight > 0.0 {
            node.decentralization_weight
        } else {
            Self::decentralization_weight_for(
                node.task_slots_granted,
                node.task_segments_completed,
                self.state.network_capacity.average_tasks_per_node,
                node.llm_profile.throughput_tok_s,
            )
        };
        let load_ratio = if self.state.network_capacity.average_tasks_per_node > 0.0 {
            node.task_slots_granted as f64
                / self.state.network_capacity.average_tasks_per_node.max(1.0)
        } else {
            1.0
        };
        let load_penalty = (1.0 / (1.0 + load_ratio)).clamp(0.35, 1.0);
        (owner_stake.sqrt() + throughput_factor) * reputation_factor * fairness * load_penalty
    }

    fn region_affinity(preferred: Option<&str>, provider_region: &str) -> f64 {
        if let Some(pref) = preferred {
            if pref.eq_ignore_ascii_case(provider_region) {
                return 1.35;
            }
            let pref_prefix = pref.split('-').next().unwrap_or(pref);
            let provider_prefix = provider_region.split('-').next().unwrap_or(provider_region);
            if pref_prefix.eq_ignore_ascii_case(provider_prefix) {
                return 1.15;
            }
            0.85
        } else {
            1.0
        }
    }

    fn estimate_latency_ms(throughput: f64, tokens: u64) -> u64 {
        if throughput <= 0.0 {
            return 0;
        }
        let seconds = (tokens as f64) / throughput.max(1.0);
        (seconds * 1000.0).ceil() as u64
    }

    /// Applies a transaction to the ledger state.
    pub fn apply_transaction(&mut self, tx: &Transaction) -> Result<(), LedgerError> {
        match &tx.kind {
            TransactionKind::Mint {
                to, token, amount, ..
            } => {
                let account = self.get_or_create_account(to);
                Self::adjust_balance(account, token, *amount as i128)?;
                account.total_earnings += amount;
            }
            TransactionKind::Burn {
                from,
                token,
                amount,
                ..
            } => {
                let account = self.get_account(from)?;
                Self::adjust_balance(account, token, -(*amount as i128))?;
                account.total_costs += amount;
            }
            TransactionKind::Transfer {
                from,
                to,
                token,
                amount,
                ..
            } => {
                let from_account = self.get_account(from)?;
                Self::adjust_balance(from_account, token, -(*amount as i128))?;
                let to_account = self.get_or_create_account(to);
                Self::adjust_balance(to_account, token, *amount as i128)?;
            }
            TransactionKind::Stake { owner, amount } => {
                let account = self.get_account(owner)?;
                Self::adjust_balance(account, &TokenKind::AIA, -(*amount as i128))?;
                account.stake_balance += amount;
            }
            TransactionKind::Airdrop {
                to, token, amount, ..
            } => {
                let account = self.get_or_create_account(to);
                Self::adjust_balance(account, token, *amount as i128)?;
                account.total_earnings += amount;
            }
            TransactionKind::Payout {
                to, token, amount, ..
            } => {
                match token {
                    TokenKind::AIA => {
                        if self.state.treasury.aia_balance < *amount {
                            return Err(LedgerError::InvalidTransaction(
                                "treasury has insufficient AIA".to_string(),
                            ));
                        }
                        self.state.treasury.aia_balance -= amount;
                    }
                    TokenKind::WORK => {
                        if self.state.treasury.work_balance < *amount {
                            return Err(LedgerError::InvalidTransaction(
                                "treasury has insufficient WORK".to_string(),
                            ));
                        }
                        self.state.treasury.work_balance -= amount;
                    }
                    TokenKind::STOR => {
                        if self.state.treasury.stor_balance < *amount {
                            return Err(LedgerError::InvalidTransaction(
                                "treasury has insufficient STOR".to_string(),
                            ));
                        }
                        self.state.treasury.stor_balance -= amount;
                    }
                }
                let account = self.get_or_create_account(to);
                Self::adjust_balance(account, token, *amount as i128)?;
                account.total_earnings += amount;
            }
            TransactionKind::SegmentReceipt {
                task_id,
                segment_id,
                provider_id,
                tokens,
                latency_ms,
                reward,
            } => {
                let task = self
                    .state
                    .tasks
                    .get_mut(task_id)
                    .ok_or_else(|| LedgerError::TaskNotFound(task_id.clone()))?;
                let provider = self
                    .state
                    .providers
                    .get(provider_id)
                    .ok_or_else(|| LedgerError::ProviderNotFound(provider_id.clone()))?;
                let mut segment = TaskSegment {
                    segment_id: segment_id.clone(),
                    provider_id: provider_id.clone(),
                    tokens: *tokens,
                    latency_ms: *latency_ms,
                    reward: *reward,
                    submitted_at: tx.timestamp,
                };
                // Apply low-tier discount dynamically.
                if provider.low_tier_discount_bps > 0 {
                    let discount =
                        (*reward as u128 * provider.low_tier_discount_bps as u128) / 10_000u128;
                    if discount > 0 {
                        segment.reward = reward.saturating_sub(discount as u64);
                    }
                }
                let treasury_fee = self.calculate_treasury_fee(segment.reward);
                segment.reward = segment.reward.saturating_sub(treasury_fee);
                self.state.treasury.aia_balance =
                    self.state.treasury.aia_balance.saturating_add(treasury_fee);
                task.segments.push(segment);
                if task.segments.len() as u64 * 100_000 >= task.total_tokens {
                    task.completed = true;
                    if let Some(best) = task.segments.iter().min_by_key(|seg| seg.latency_ms) {
                        task.winning_provider = Some(best.provider_id.clone());
                    }
                }
            }
            TransactionKind::ChatResult {
                task_id,
                aggregate,
                contributors,
            } => {
                let task = self
                    .state
                    .tasks
                    .get_mut(task_id)
                    .ok_or_else(|| LedgerError::TaskNotFound(task_id.clone()))?;
                task.chat_aggregate = Some(aggregate.clone());
                task.completed = true;
                if task.winning_provider.is_none() {
                    task.winning_provider = contributors.first().cloned();
                }
            }
            TransactionKind::TaskProofCommit { proof } => {
                self.record_task_proof(proof.clone())?;
            }
        }
        Ok(())
    }

    /// Appends a block and persists the ledger.
    pub fn commit_block(&mut self, transactions: Vec<Transaction>) -> Result<Block, LedgerError> {
        for tx in &transactions {
            self.apply_transaction(tx)?;
        }
        let height = self.state.blocks.len() as u64;
        let block = Block {
            height,
            timestamp: current_timestamp(),
            transactions: transactions.clone(),
        };
        self.state.blocks.push(block.clone());
        self.persist()?;
        info!(
            height,
            tx_count = block.transactions.len(),
            "committed block"
        );
        Ok(block)
    }

    /// Registers or updates a provider.
    pub fn upsert_provider(&mut self, mut provider: Provider) {
        let baseline = provider
            .model_profiles
            .iter()
            .map(|profile| profile.throughput_tok_s as f64)
            .fold(0.0, |acc, next| acc.max(next))
            .max((provider.bandwidth_gbps as f64) * 1_000.0);
        provider.throughput_score = provider.throughput_score.max(baseline);
        self.state.providers.insert(provider.id.clone(), provider);
    }

    /// Updates model profiles for a provider.
    pub fn set_provider_models(
        &mut self,
        provider_id: &str,
        profiles: Vec<ModelProfile>,
    ) -> Result<(), LedgerError> {
        let provider = self
            .state
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| LedgerError::ProviderNotFound(provider_id.to_string()))?;
        provider.model_profiles = profiles;
        let baseline = provider
            .model_profiles
            .iter()
            .map(|profile| profile.throughput_tok_s as f64)
            .fold(0.0, |acc, next| acc.max(next))
            .max((provider.bandwidth_gbps as f64) * 1_000.0);
        provider.throughput_score = provider.throughput_score.max(baseline);
        Ok(())
    }

    /// Registers a node in the ledger and returns the stored record.
    pub fn register_node(&mut self, mut node: Node) -> Node {
        node.registered_at = current_timestamp();
        node.id = format!("node-{}", self.state.next_node_id);
        self.state.next_node_id += 1;
        if node.fingerprint.is_empty() {
            node.fingerprint = Self::fingerprint_for(&node.hardware, &node.owner);
        }
        if node.metrics.timestamp == 0 {
            node.metrics.timestamp = node.registered_at;
        }
        node.task_slots_granted = 0;
        node.task_segments_completed = 0;
        node.decentralization_weight = Self::decentralization_weight_for(
            node.task_slots_granted,
            node.task_segments_completed,
            self.state.network_capacity.average_tasks_per_node,
            node.llm_profile.throughput_tok_s,
        );
        self.state.nodes.insert(node.id.clone(), node.clone());
        self.recalculate_average_tasks();
        node
    }

    fn fingerprint_for(hardware: &NodeHardware, owner: &str) -> String {
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

    /// Submits a new task and returns the identifier.
    pub fn submit_task(
        &mut self,
        owner: String,
        content_hash: String,
        total_tokens: u64,
        target_profile: ModelProfile,
        providers: Vec<String>,
        preferred_region: Option<String>,
        mode: TaskMode,
        chat_prompt: Option<String>,
    ) -> String {
        let id = format!("task-{}", self.state.next_task_id);
        self.state.next_task_id += 1;
        let mut task = Task {
            id: id.clone(),
            owner,
            content_hash,
            total_tokens,
            target_profile,
            requested_providers: providers,
            segments: Vec::new(),
            created_at: current_timestamp(),
            completed: false,
            winning_provider: None,
            preferred_region,
            scheduled: Vec::new(),
            mode,
            chat_prompt,
            chat_responses: Vec::new(),
            chat_aggregate: None,
        };
        task.scheduled = self.compute_scheduled_providers(&task);
        self.state.tasks.insert(id.clone(), task);
        id
    }

    /// Returns a reference snapshot of the state for read-only use.
    pub fn snapshot(&self) -> LedgerState {
        self.state.clone()
    }

    /// Computes the treasury fee portion for a segment payout.
    pub fn calculate_treasury_fee(&self, amount: u64) -> u64 {
        (amount as u128 * self.state.treasury.fee_bps as u128 / 10_000u128) as u64
    }

    /// Credits block rewards to stakers based on their proportional stake.
    pub fn distribute_block_rewards(&mut self) -> Result<Vec<Transaction>, LedgerError> {
        let total_stake: u64 = self
            .state
            .accounts
            .values()
            .map(|acct| acct.stake_balance)
            .sum();
        if total_stake == 0 {
            return Ok(Vec::new());
        }
        let reward_pool = (self.state.treasury.aia_balance as u128
            * self.state.treasury.reward_bps as u128
            / 10_000u128) as u64;
        if reward_pool == 0 {
            return Ok(Vec::new());
        }
        self.state.treasury.aia_balance =
            self.state.treasury.aia_balance.saturating_sub(reward_pool);
        let now = current_timestamp();
        let mut payouts = Vec::new();
        for account in self.state.accounts.values() {
            if account.stake_balance == 0 {
                continue;
            }
            let share =
                (reward_pool as u128 * account.stake_balance as u128 / total_stake as u128) as u64;
            if share == 0 {
                continue;
            }
            let tx = Transaction {
                id: format!("block-reward-{}-{}", now, account.id),
                timestamp: now,
                kind: TransactionKind::Payout {
                    to: account.id.clone(),
                    token: TokenKind::AIA,
                    amount: share,
                    task_id: "block_reward".into(),
                },
            };
            payouts.push(tx);
        }
        Ok(payouts)
    }

    /// Records an operational heartbeat for providers to indicate liveness.
    pub fn heartbeat_provider(&mut self, provider_id: &str) -> Result<(), LedgerError> {
        let provider = self
            .state
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| LedgerError::ProviderNotFound(provider_id.to_string()))?;
        provider.last_heartbeat = Some(current_timestamp());
        Ok(())
    }

    /// Updates a node's online status and optionally refreshes metrics.
    pub fn update_node_status(
        &mut self,
        node_id: &str,
        online: bool,
        metrics: Option<NodeMetrics>,
    ) -> Result<(), LedgerError> {
        let node = self
            .state
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| LedgerError::NodeNotFound(node_id.to_string()))?;
        node.online = online;
        if let Some(mut report) = metrics {
            if report.timestamp == 0 {
                report.timestamp = current_timestamp();
            }
            node.metrics = report;
        }
        Ok(())
    }

    /// Returns the top providers for a task leveraging throughput, pricing, stake and region bias.
    pub fn compute_scheduled_providers(&self, task: &Task) -> Vec<ScheduledProvider> {
        let mut decisions = Vec::new();
        let preferred = task.preferred_region.as_deref();
        let candidates: Vec<&Provider> = if task.requested_providers.is_empty() {
            self.state.providers.values().collect()
        } else {
            task.requested_providers
                .iter()
                .filter_map(|id| self.state.providers.get(id))
                .collect()
        };
        for provider in candidates {
            let throughput = Self::provider_peak_throughput(provider);
            if throughput <= 0.0 {
                continue;
            }
            let region_factor = Self::region_affinity(preferred, &provider.region);
            let stake_factor = 1.0 + provider.stake_boost.max(0.0);
            let reputation_factor = 1.0 + (provider.reputation as f64 / 100.0);
            let price = provider
                .pricing
                .current_per_token
                .max(provider.pricing.base_per_token)
                .max(1);
            let price_factor = 1.0 / (price as f64);
            let bandwidth_factor = (provider.bandwidth_gbps as f64).max(1.0).sqrt();
            let score = throughput
                * region_factor
                * stake_factor
                * reputation_factor
                * price_factor
                * bandwidth_factor;
            if !score.is_finite() {
                continue;
            }
            let expected_latency = Self::estimate_latency_ms(throughput, task.total_tokens);
            decisions.push(ScheduledProvider {
                provider_id: provider.id.clone(),
                score,
                expected_latency_ms: expected_latency,
                max_tokens_per_sec: throughput as u64,
            });
        }
        decisions.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        decisions.truncate(8);
        decisions
    }

    fn truncate_chat_snippet(text: &str) -> String {
        let trimmed = text.trim();
        const MAX: usize = 240;
        if trimmed.len() <= MAX {
            return trimmed.to_string();
        }
        let mut snippet = trimmed.chars().take(MAX).collect::<String>();
        snippet.push('…');
        snippet
    }

    fn aggregate_chat_responses(responses: &[ChatResponse]) -> String {
        if responses.is_empty() {
            return String::new();
        }
        let mut sorted = responses.to_vec();
        sorted.sort_by_key(|resp| resp.latency_ms);
        let highlights: Vec<String> = sorted
            .iter()
            .take(3)
            .enumerate()
            .map(|(idx, resp)| {
                format!(
                    "{}. [{} ms | {} tok | {}] {}",
                    idx + 1,
                    resp.latency_ms,
                    resp.tokens,
                    resp.provider_id,
                    Self::truncate_chat_snippet(&resp.response_fragment)
                )
            })
            .collect();
        let unique_providers: HashSet<&str> = responses
            .iter()
            .map(|resp| resp.provider_id.as_str())
            .collect();
        let mut summary = format!(
            "Aggregated {} chat response{} from {} provider{}.",
            responses.len(),
            if responses.len() == 1 { "" } else { "s" },
            unique_providers.len(),
            if unique_providers.len() == 1 { "" } else { "s" }
        );
        if !highlights.is_empty() {
            summary.push('\n');
            summary.push_str(&highlights.join("\n"));
        }
        summary
    }

    fn integrate_chat_results(
        &mut self,
        task_id: &str,
        proofs: &[TaskProof],
    ) -> Option<Transaction> {
        let task = self.state.tasks.get_mut(task_id)?;
        if task.mode != TaskMode::Chat {
            return None;
        }
        let mut existing_segments: HashSet<String> = HashSet::new();
        for response in &task.chat_responses {
            existing_segments.insert(response.segment_id.clone());
        }
        let mut new_entries: Vec<ChatResponse> = Vec::new();
        for proof in proofs {
            let response_text = match proof.chat_response.as_ref() {
                Some(text) if !text.trim().is_empty() => text.trim().to_string(),
                _ => continue,
            };
            if existing_segments.contains(&proof.segment_id) {
                continue;
            }
            existing_segments.insert(proof.segment_id.clone());
            new_entries.push(ChatResponse {
                provider_id: proof.provider_id.clone(),
                node_id: proof.node_id.clone(),
                segment_id: proof.segment_id.clone(),
                latency_ms: proof.latency_ms,
                tokens: proof.tokens_processed,
                response_fragment: response_text,
                submitted_at: 0,
            });
        }
        if new_entries.is_empty() {
            return None;
        }
        let timestamp = current_timestamp();
        for mut entry in new_entries {
            entry.submitted_at = timestamp;
            task.chat_responses.push(entry);
        }
        let aggregate = Self::aggregate_chat_responses(&task.chat_responses);
        task.chat_aggregate = Some(aggregate.clone());
        task.completed = true;
        let mut contributors = task
            .chat_responses
            .iter()
            .map(|resp| resp.provider_id.clone())
            .collect::<Vec<_>>();
        contributors.sort();
        contributors.dedup();
        Some(Transaction {
            id: format!("chat-result-{}-{}", task_id, timestamp),
            timestamp,
            kind: TransactionKind::ChatResult {
                task_id: task_id.to_string(),
                aggregate,
                contributors,
            },
        })
    }

    fn record_task_proof(&mut self, proof: TaskProof) -> Result<(), LedgerError> {
        if !self.state.tasks.contains_key(&proof.task_id) {
            return Err(LedgerError::TaskNotFound(proof.task_id.clone()));
        }
        let provider = self
            .state
            .providers
            .get_mut(&proof.provider_id)
            .ok_or_else(|| LedgerError::ProviderNotFound(proof.provider_id.clone()))?;
        if proof.round < self.state.network_capacity.consensus_round {
            return Err(LedgerError::InvalidTransaction(format!(
                "stale task-proof round {} < {}",
                proof.round, self.state.network_capacity.consensus_round
            )));
        }
        self.state.pending_task_proofs.retain(|existing| {
            !(existing.task_id == proof.task_id
                && existing.segment_id == proof.segment_id
                && existing.provider_id == proof.provider_id)
        });
        provider.reputation = provider
            .reputation
            .saturating_add((proof.tokens_processed / 1_000_000).max(1));
        provider.throughput_score =
            (provider.throughput_score * 0.7) + (proof.throughput_tok_s as f64 * 0.3);
        self.state.pending_task_proofs.push(proof.clone());
        self.state.network_capacity.tokens_processed += proof.tokens_processed as u128;
        self.state.network_capacity.consensus_round =
            self.state.network_capacity.consensus_round.max(proof.round);
        if proof.throughput_tok_s > self.state.network_capacity.peak_observed_tokens_per_sec {
            self.state.network_capacity.peak_observed_tokens_per_sec = proof.throughput_tok_s;
        }
        Ok(())
    }

    fn calculate_segment_reward(&self, proof: &TaskProof) -> u64 {
        let mut base_reward = 0u64;
        if let Some(provider) = self.state.providers.get(&proof.provider_id) {
            let per_token = provider
                .pricing
                .current_per_token
                .max(provider.pricing.base_per_token)
                .max(1);
            base_reward = per_token.saturating_mul(proof.tokens_processed);
            let boost = 1.0 + provider.stake_boost.max(0.0);
            base_reward = (base_reward as f64 * boost).round() as u64;
        }
        if base_reward == 0 {
            base_reward = proof.tokens_processed.max(1);
        }
        if let Some(task) = self.state.tasks.get(&proof.task_id) {
            if let Some(preferred) = task.preferred_region.as_ref() {
                if preferred.eq_ignore_ascii_case(&proof.region) {
                    base_reward = (base_reward as f64 * 1.1).round() as u64;
                }
            }
        }
        base_reward
    }

    pub fn collect_task_proof_certificates(&mut self) -> Vec<Transaction> {
        if self.state.pending_task_proofs.is_empty() {
            return Vec::new();
        }
        let mut grouped: HashMap<String, Vec<TaskProof>> = HashMap::new();
        for proof in std::mem::take(&mut self.state.pending_task_proofs) {
            grouped
                .entry(proof.task_id.clone())
                .or_default()
                .push(proof);
        }
        let mut outputs = Vec::new();
        for (task_id, mut proofs) in grouped {
            if proofs.is_empty() {
                continue;
            }
            if let Some(chat_result) = self.integrate_chat_results(&task_id, &proofs) {
                outputs.push(chat_result);
            }
            proofs.sort_by_key(|proof| proof.latency_ms);
            if let Some(best) = proofs.first() {
                let reward = self.calculate_segment_reward(best);
                if let Some(node_id) = best.node_id.as_deref() {
                    self.apply_task_share_for_node(node_id);
                }
                let receipt = Transaction {
                    id: format!("segment-{}-{}", task_id, best.segment_id),
                    timestamp: current_timestamp(),
                    kind: TransactionKind::SegmentReceipt {
                        task_id: task_id.clone(),
                        segment_id: best.segment_id.clone(),
                        provider_id: best.provider_id.clone(),
                        tokens: best.tokens_processed,
                        latency_ms: best.latency_ms,
                        reward,
                    },
                };
                outputs.push(receipt);
                let payout = Transaction {
                    id: format!("payout-{}-{}", task_id, best.provider_id),
                    timestamp: current_timestamp(),
                    kind: TransactionKind::Payout {
                        to: best.provider_id.clone(),
                        token: TokenKind::AIA,
                        amount: reward,
                        task_id: task_id.clone(),
                    },
                };
                outputs.push(payout);
                if let Some(task) = self.state.tasks.get_mut(&task_id) {
                    task.winning_provider = Some(best.provider_id.clone());
                    if task.segments.is_empty() {
                        task.scheduled = self.compute_scheduled_providers(task);
                    }
                }
            }
        }
        self.state.network_capacity.consensus_round += 1;
        outputs
    }

    /// Selects a leader node for the next task-proof consensus round.
    pub fn select_task_proof_leader(&self) -> Option<String> {
        let mut best: Option<(String, f64)> = None;
        for node in self.state.nodes.values().filter(|n| n.online) {
            let owner_stake = self
                .state
                .accounts
                .get(&node.owner)
                .map(|acct| acct.stake_balance as f64)
                .unwrap_or(0.0);
            let score = self.consensus_score_for_node(node, owner_stake);
            if let Some((_, best_score)) = &best {
                if score > *best_score {
                    best = Some((node.id.clone(), score));
                }
            } else {
                best = Some((node.id.clone(), score));
            }
        }
        best.map(|(id, _)| id)
    }
}

/// Metadata returned by the RPC service for lightweight summaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSummary {
    pub block_height: u64,
    pub treasury_aia: u64,
    pub active_providers: usize,
    pub active_nodes: usize,
    pub total_tasks: usize,
    pub consensus_round: u64,
    pub target_tokens_per_sec: u64,
    pub peak_tokens_per_sec: u64,
    pub total_tokens_processed: u128,
    pub max_parallel_nodes: u64,
    pub total_task_slots: u128,
    pub average_tasks_per_node: f64,
}

impl From<&LedgerState> for NetworkSummary {
    fn from(state: &LedgerState) -> Self {
        let active_providers = state
            .providers
            .values()
            .filter(|provider| provider.last_heartbeat.is_some())
            .count();
        let active_nodes = state.nodes.values().filter(|node| node.online).count();
        Self {
            block_height: state.blocks.len() as u64,
            treasury_aia: state.treasury.aia_balance,
            active_providers,
            active_nodes,
            total_tasks: state.tasks.len(),
            consensus_round: state.network_capacity.consensus_round,
            target_tokens_per_sec: state.network_capacity.target_tokens_per_sec,
            peak_tokens_per_sec: state.network_capacity.peak_observed_tokens_per_sec,
            total_tokens_processed: state.network_capacity.tokens_processed,
            max_parallel_nodes: state.network_capacity.max_parallel_nodes,
            total_task_slots: state.network_capacity.total_task_slots,
            average_tasks_per_node: state.network_capacity.average_tasks_per_node,
        }
    }
}
