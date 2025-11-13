//! GPANG Network ledger module.
//!
//! The ledger owns all on-chain state including accounts, GPU providers, nodes,
//! and task execution records. State is persisted as JSON on disk so that the
//! entire blockchain can operate without an external database. All mutating
//! operations funnel through [`Ledger::apply_transaction`] to provide a single
//! entrypoint for token and reward accounting.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
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
    Account, Block, ChatResponse, ContractEvent, ContractRuntime, LedgerState, ModelProfile, Node,
    NodeHardware, NodeMetrics, NodeRole, Provider, ScheduledProvider, SmartContract, Task,
    TaskMode, TaskProof, TaskSegment, TokenDefinition, TokenKind, Transaction, TransactionKind,
    ZeroProof,
};

/// Default scheme label for synthesized zero proofs.
pub const ZERO_PROOF_SCHEME: &str = "gpang-zero-proof";

/// Minimum gas price accepted by the ledger.
pub const MIN_GAS_PRICE: u64 = 1;

/// Minimum stake (in AIA) required for validator role registration.
pub const MIN_VALIDATOR_STAKE: u64 = 100_000_000;

/// Returns the current Unix timestamp in milliseconds.
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Default persistence file name.
pub const DEFAULT_LEDGER_FILE: &str = "ledger_v35.json";

/// Returns the intrinsic gas cost for the provided transaction kind.
pub fn intrinsic_gas_cost(kind: &TransactionKind) -> u64 {
    match kind {
        TransactionKind::Mint { .. } => 21_000,
        TransactionKind::Burn { .. } => 20_000,
        TransactionKind::Transfer { .. } => 25_000,
        TransactionKind::Stake { .. } => 28_000,
        TransactionKind::Airdrop { .. } => 22_000,
        TransactionKind::Payout { .. } => 24_000,
        TransactionKind::SegmentReceipt { .. } => 40_000,
        TransactionKind::ChatResult { .. } => 36_000,
        TransactionKind::TaskProofCommit { .. } => 55_000,
        TransactionKind::DeployContract { .. } => 120_000,
        TransactionKind::ExecuteContract { .. } => 65_000,
        TransactionKind::CreateToken { .. } => 110_000,
        TransactionKind::MintCustom { .. } => 42_000,
        TransactionKind::TransferCustom { .. } => 32_000,
        TransactionKind::TreasuryDeposit { .. } => 30_000,
        TransactionKind::TreasuryWithdraw { .. } => 45_000,
    }
}

fn zero_proof_statement(
    kind: &TransactionKind,
    gas_payer: Option<&str>,
    gas_limit: u64,
    gas_price: u64,
) -> String {
    serde_json::to_string(&serde_json::json!({
        "kind": kind,
        "gas_payer": gas_payer,
        "gas_limit": gas_limit,
        "gas_price": gas_price
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

/// Synthesizes a deterministic zero proof for the transaction metadata.
pub fn synthesize_zero_proof(
    kind: &TransactionKind,
    gas_payer: Option<&str>,
    gas_limit: u64,
    gas_price: u64,
) -> ZeroProof {
    let statement = zero_proof_statement(kind, gas_payer, gas_limit, gas_price);
    ZeroProof::new(ZERO_PROOF_SCHEME, statement)
}

/// Helper for constructing transactions with canonical gas and zero proof metadata.
pub fn build_transaction(
    id: String,
    timestamp: u64,
    kind: TransactionKind,
    gas_payer: Option<String>,
    gas_price: u64,
    gas_limit: Option<u64>,
) -> Transaction {
    let intrinsic = intrinsic_gas_cost(&kind);
    let mut limit = gas_limit.unwrap_or(intrinsic);
    if limit < intrinsic {
        limit = intrinsic;
    }
    let price = gas_price.max(MIN_GAS_PRICE);
    let proof = synthesize_zero_proof(&kind, gas_payer.as_deref(), limit, price);
    Transaction {
        id,
        timestamp,
        kind,
        gas_payer,
        gas_limit: limit,
        gas_price: price,
        gas_used: intrinsic,
        zero_proof: Some(proof),
    }
}

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
    #[error("token {0} not found")]
    TokenNotFound(String),
    #[error("contract {0} not found")]
    ContractNotFound(String),
    #[error("forbidden operation: {0}")]
    Forbidden(String),
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

    fn canonical_symbol(symbol: &str) -> String {
        symbol.trim().to_ascii_uppercase()
    }

    fn is_builtin_symbol(symbol: &str) -> Option<TokenKind> {
        match symbol {
            "AIA" => Some(TokenKind::AIA),
            "WORK" => Some(TokenKind::WORK),
            "STOR" => Some(TokenKind::STOR),
            _ => None,
        }
    }

    fn adjust_custom_balance(
        account: &mut Account,
        symbol: &str,
        delta: i128,
    ) -> Result<(), LedgerError> {
        let entry = account
            .custom_tokens
            .entry(Self::canonical_symbol(symbol))
            .or_insert(0);
        let new_value = *entry as i128 + delta;
        if new_value < 0 {
            return Err(LedgerError::InsufficientBalance(format!(
                "{} for account {}",
                symbol, account.id
            )));
        }
        *entry = new_value as u64;
        Ok(())
    }

    fn adjust_named_balance(
        &mut self,
        account: &mut Account,
        symbol: &str,
        delta: i128,
    ) -> Result<(), LedgerError> {
        if let Some(kind) = Self::is_builtin_symbol(symbol) {
            Self::adjust_balance(account, &kind, delta)
        } else {
            if !self
                .state
                .token_definitions
                .contains_key(&Self::canonical_symbol(symbol))
            {
                return Err(LedgerError::TokenNotFound(symbol.to_string()));
            }
            Self::adjust_custom_balance(account, symbol, delta)
        }
    }

    fn adjust_treasury(&mut self, symbol: &str, delta: i128) -> Result<(), LedgerError> {
        if let Some(kind) = Self::is_builtin_symbol(symbol) {
            let balance = match kind {
                TokenKind::AIA => &mut self.state.treasury.aia_balance,
                TokenKind::WORK => &mut self.state.treasury.work_balance,
                TokenKind::STOR => &mut self.state.treasury.stor_balance,
            };
            let new_value = *balance as i128 + delta;
            if new_value < 0 {
                return Err(LedgerError::InsufficientBalance(format!(
                    "treasury {}",
                    symbol
                )));
            }
            *balance = new_value as u64;
            Ok(())
        } else {
            let entry = self
                .state
                .treasury
                .custom_tokens
                .entry(Self::canonical_symbol(symbol))
                .or_insert(0);
            let new_value = *entry as i128 + delta;
            if new_value < 0 {
                return Err(LedgerError::InsufficientBalance(format!(
                    "treasury {}",
                    symbol
                )));
            }
            *entry = new_value as u64;
            Ok(())
        }
    }

    fn token_definition(&self, symbol: &str) -> Result<&TokenDefinition, LedgerError> {
        let key = Self::canonical_symbol(symbol);
        self.state
            .token_definitions
            .get(&key)
            .ok_or_else(|| LedgerError::TokenNotFound(symbol.to_string()))
    }

    fn token_definition_mut(&mut self, symbol: &str) -> Result<&mut TokenDefinition, LedgerError> {
        let key = Self::canonical_symbol(symbol);
        self.state
            .token_definitions
            .get_mut(&key)
            .ok_or_else(|| LedgerError::TokenNotFound(symbol.to_string()))
    }

    fn next_contract_identifier(&mut self) -> String {
        let id = format!("contract-{}", self.state.next_contract_id);
        self.state.next_contract_id += 1;
        id
    }

    fn bump_contract_counter_from(&mut self, contract_id: &str) {
        if let Some(tail) = contract_id.strip_prefix("contract-") {
            if let Ok(num) = tail.parse::<u64>() {
                if num >= self.state.next_contract_id {
                    self.state.next_contract_id = num + 1;
                }
            }
        }
    }

    fn hash_components(parts: &[&str]) -> String {
        use std::collections::hash_map::DefaultHasher;

        let mut hasher = DefaultHasher::new();
        for part in parts {
            part.hash(&mut hasher);
        }
        format!("{:016x}", hasher.finish())
    }

    fn record_contract_event(&mut self, mut event: ContractEvent) {
        if event.id.is_empty() {
            let idx = self.state.contract_events.len() as u64 + 1;
            event.id = format!("event-{}-{}", event.contract_id, idx);
        }
        self.state.contract_events.push(event);
    }

    fn ensure_zero_proof(&self, tx: &Transaction) -> Result<(), LedgerError> {
        let proof = tx
            .zero_proof
            .as_ref()
            .ok_or_else(|| LedgerError::InvalidTransaction("missing zero proof".into()))?;
        let expected = zero_proof_statement(
            &tx.kind,
            tx.gas_payer.as_deref(),
            tx.gas_limit,
            tx.gas_price,
        );
        if proof.statement != expected || !proof.verify() {
            return Err(LedgerError::InvalidTransaction(
                "zero proof verification failed".into(),
            ));
        }
        Ok(())
    }

    fn charge_gas(&mut self, tx: &Transaction) -> Result<(), LedgerError> {
        self.ensure_zero_proof(tx)?;
        if tx.gas_limit == 0 {
            return Err(LedgerError::InvalidTransaction(
                "gas limit must be greater than zero".into(),
            ));
        }
        if tx.gas_price < MIN_GAS_PRICE {
            return Err(LedgerError::InvalidTransaction(format!(
                "gas price {} below minimum {}",
                tx.gas_price, MIN_GAS_PRICE
            )));
        }
        let intrinsic = intrinsic_gas_cost(&tx.kind);
        if tx.gas_limit < intrinsic {
            return Err(LedgerError::InvalidTransaction(format!(
                "gas limit {} below intrinsic {}",
                tx.gas_limit, intrinsic
            )));
        }
        let gas_used = intrinsic;
        let fee = gas_used.saturating_mul(tx.gas_price);
        let payer = tx
            .gas_payer
            .as_ref()
            .ok_or_else(|| LedgerError::InvalidTransaction("missing gas payer".into()))?;
        let account = self.get_or_create_account(payer);
        Self::adjust_balance(account, &TokenKind::AIA, -(fee as i128))?;
        self.state.treasury.aia_balance = self.state.treasury.aia_balance.saturating_add(fee);
        self.state.treasury.gas_collected = self.state.treasury.gas_collected.saturating_add(fee);
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
        let active = self
            .state
            .nodes
            .values()
            .filter(|node| node.online && node.role == NodeRole::Compute)
            .count() as f64;
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

    fn apply_task_share_for_node(&mut self, node_id: &str, reward: u64) {
        {
            let Some(node) = self.state.nodes.get_mut(node_id) else {
                return;
            };
            if node.role != NodeRole::Compute {
                return;
            }
            node.task_slots_granted = node.task_slots_granted.saturating_add(1);
            node.task_segments_completed = node.task_segments_completed.saturating_add(1);
            node.total_rewards = node.total_rewards.saturating_add(reward);
            node.last_reward_at = current_timestamp();
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
        if node.role != NodeRole::Validator {
            return 0.0;
        }
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

    fn select_role_node(&self, preferred_region: Option<&str>, role: NodeRole) -> Option<String> {
        let mut best_id: Option<String> = None;
        let mut best_score = f64::MIN;
        for node in self.state.nodes.values() {
            if node.role != role || !node.online {
                continue;
            }
            let region_factor = match preferred_region {
                Some(pref) if pref == node.region => 1.5,
                Some(pref) => {
                    let pref_prefix = pref.split('-').next().unwrap_or(pref);
                    let node_prefix = node
                        .region
                        .split('-')
                        .next()
                        .unwrap_or_else(|| node.region.as_str());
                    if pref_prefix == node_prefix {
                        1.2
                    } else {
                        1.0
                    }
                }
                None => 1.0,
            };
            let load = (node.metrics.machine_load_one as f64).max(0.1);
            let backlog = match role {
                NodeRole::Scheduler => node.scheduler_jobs_executed as f64,
                NodeRole::Assignment => node.assignment_jobs_generated as f64,
                _ => node.task_slots_granted as f64,
            };
            let backlog_factor = 1.0 / (1.0 + backlog);
            let score = region_factor * backlog_factor / load;
            if score > best_score {
                best_score = score;
                best_id = Some(node.id.clone());
            }
        }
        best_id
    }

    fn select_assignment_node(&self, preferred_region: Option<&str>) -> Option<String> {
        self.select_role_node(preferred_region, NodeRole::Assignment)
    }

    fn select_scheduler_node(&self, preferred_region: Option<&str>) -> Option<String> {
        self.select_role_node(preferred_region, NodeRole::Scheduler)
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
            TransactionKind::DeployContract {
                owner,
                contract_id,
                name,
                code,
                metadata,
                runtime,
                program_id,
                bytecode_b64,
            } => {
                let mut id = contract_id.clone().unwrap_or_default();
                if id.trim().is_empty() {
                    id = self.next_contract_identifier();
                } else if self.state.contracts.contains_key(&id) {
                    return Err(LedgerError::InvalidTransaction(format!(
                        "contract {} already exists",
                        id
                    )));
                } else {
                    self.bump_contract_counter_from(&id);
                }

                let mut source = code.clone().unwrap_or_default();
                let mut bytecode = bytecode_b64.clone();
                let mut normalized_program = program_id.clone();

                match runtime {
                    ContractRuntime::Native => {
                        if source.trim().is_empty() {
                            return Err(LedgerError::InvalidTransaction(
                                "native contract code cannot be empty".into(),
                            ));
                        }
                        normalized_program = None;
                        bytecode = None;
                    }
                    ContractRuntime::Solana => {
                        let provided = program_id.as_ref().ok_or_else(|| {
                            LedgerError::InvalidTransaction(
                                "solana deployments require program_id".into(),
                            )
                        })?;
                        let trimmed = provided.trim();
                        if trimmed.is_empty() {
                            return Err(LedgerError::InvalidTransaction(
                                "solana deployments require a non-empty program_id".into(),
                            ));
                        }
                        normalized_program = Some(trimmed.to_string());
                        if bytecode.is_none() {
                            if source.trim().is_empty() {
                                return Err(LedgerError::InvalidTransaction(
                                    "solana deployments require base64 bytecode".into(),
                                ));
                            }
                            bytecode = Some(source.clone());
                            source.clear();
                        }
                        let encoded = bytecode
                            .as_ref()
                            .expect("bytecode ensured for solana deployments");
                        if encoded.trim().is_empty() {
                            return Err(LedgerError::InvalidTransaction(
                                "solana bytecode cannot be empty".into(),
                            ));
                        }
                        if BASE64.decode(encoded.as_bytes()).is_err() {
                            return Err(LedgerError::InvalidTransaction(
                                "solana bytecode must be valid base64".into(),
                            ));
                        }
                    }
                }

                let material_ref = if matches!(runtime, ContractRuntime::Solana) {
                    bytecode
                        .as_ref()
                        .map(|s| s.as_str())
                        .unwrap_or_else(|| source.as_str())
                } else {
                    source.as_str()
                };
                let mut hash_parts: Vec<&str> = vec![id.as_str(), runtime.as_str(), material_ref];
                if let Some(ref pid) = normalized_program {
                    hash_parts.push(pid.as_str());
                }
                let code_hash = Self::hash_components(&hash_parts);
                let contract = SmartContract {
                    id: id.clone(),
                    owner: owner.clone(),
                    name: name.clone(),
                    code: source.clone(),
                    code_hash,
                    metadata: metadata.clone(),
                    deployed_at: tx.timestamp,
                    runtime: runtime.clone(),
                    program_id: normalized_program.clone(),
                    bytecode_b64: bytecode.clone(),
                };
                self.state.contracts.insert(id, contract);
                self.get_or_create_account(owner);
            }
            TransactionKind::ExecuteContract {
                contract_id,
                caller,
                method,
                payload,
            } => {
                if !self.state.contracts.contains_key(contract_id) {
                    return Err(LedgerError::ContractNotFound(contract_id.clone()));
                }
                let payload_str =
                    serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
                let payload_hash = Self::hash_components(&[contract_id, &payload_str, method]);
                let event = ContractEvent {
                    id: String::new(),
                    contract_id: contract_id.clone(),
                    caller: caller.clone(),
                    method: method.clone(),
                    payload: payload.clone(),
                    payload_hash,
                    timestamp: tx.timestamp,
                };
                self.record_contract_event(event);
            }
            TransactionKind::CreateToken {
                symbol,
                name,
                decimals,
                initial_supply,
                owner,
                platform,
                contract_id,
            } => {
                let symbol = Self::canonical_symbol(symbol);
                if Self::is_builtin_symbol(&symbol).is_some() {
                    return Err(LedgerError::InvalidTransaction(
                        "cannot redefine builtin token".into(),
                    ));
                }
                if self.state.token_definitions.contains_key(&symbol) {
                    return Err(LedgerError::InvalidTransaction(format!(
                        "token {} already exists",
                        symbol
                    )));
                }
                if *decimals > 18 {
                    return Err(LedgerError::InvalidTransaction(
                        "decimals must be <= 18".into(),
                    ));
                }
                if let Some(contract) = contract_id {
                    if !self.state.contracts.contains_key(contract) {
                        return Err(LedgerError::ContractNotFound(contract.clone()));
                    }
                }
                let definition = TokenDefinition {
                    symbol: symbol.clone(),
                    name: name.clone(),
                    decimals: *decimals,
                    total_supply: *initial_supply as u128,
                    owner: owner.clone(),
                    platform: *platform,
                    created_at: tx.timestamp,
                    contract_id: contract_id.clone(),
                };
                self.state
                    .token_definitions
                    .insert(symbol.clone(), definition);
                if *platform {
                    self.state.platform_tokens.insert(symbol.clone());
                    self.adjust_treasury(&symbol, *initial_supply as i128)?;
                } else {
                    let account = self.get_or_create_account(owner);
                    Self::adjust_custom_balance(account, &symbol, *initial_supply as i128)?;
                }
                self.get_or_create_account(owner);
            }
            TransactionKind::MintCustom {
                symbol,
                to,
                amount,
                authority,
            } => {
                let symbol = Self::canonical_symbol(symbol);
                let owner_id = {
                    let definition = self.token_definition(&symbol)?;
                    definition.owner.clone()
                };
                if &owner_id != authority {
                    return Err(LedgerError::Forbidden(format!(
                        "{} cannot mint {}",
                        authority, symbol
                    )));
                }
                let definition = self.token_definition_mut(&symbol)?;
                definition.total_supply = definition.total_supply.saturating_add(*amount as u128);
                let account = self.get_or_create_account(to);
                Self::adjust_custom_balance(account, &symbol, *amount as i128)?;
            }
            TransactionKind::TransferCustom {
                symbol,
                from,
                to,
                amount,
            } => {
                let symbol = Self::canonical_symbol(symbol);
                self.token_definition(&symbol)?;
                let from_account = self.get_account(from)?;
                Self::adjust_custom_balance(from_account, &symbol, -(*amount as i128))?;
                let to_account = self.get_or_create_account(to);
                Self::adjust_custom_balance(to_account, &symbol, *amount as i128)?;
            }
            TransactionKind::TreasuryDeposit {
                from,
                symbol,
                amount,
            } => {
                let symbol = Self::canonical_symbol(symbol);
                if let Some(kind) = Self::is_builtin_symbol(&symbol) {
                    let account = self.get_account(from)?;
                    Self::adjust_balance(account, &kind, -(*amount as i128))?;
                    self.adjust_treasury(&symbol, *amount as i128)?;
                } else {
                    self.token_definition(&symbol)?;
                    let account = self.get_account(from)?;
                    Self::adjust_custom_balance(account, &symbol, -(*amount as i128))?;
                    self.adjust_treasury(&symbol, *amount as i128)?;
                }
            }
            TransactionKind::TreasuryWithdraw {
                to,
                symbol,
                amount,
                authority,
            } => {
                let symbol = Self::canonical_symbol(symbol);
                if let Some(kind) = Self::is_builtin_symbol(&symbol) {
                    let authority_account = self.get_account(authority)?;
                    if authority_account.stake_balance == 0 {
                        return Err(LedgerError::Forbidden(
                            "treasury withdrawal requires staker authority".into(),
                        ));
                    }
                    self.adjust_treasury(&symbol, -(*amount as i128))?;
                    let to_account = self.get_or_create_account(to);
                    Self::adjust_balance(to_account, &kind, *amount as i128)?;
                } else {
                    let owner_id = {
                        let definition = self.token_definition(&symbol)?;
                        definition.owner.clone()
                    };
                    if &owner_id != authority {
                        return Err(LedgerError::Forbidden(format!(
                            "{} cannot withdraw {} from treasury",
                            authority, symbol
                        )));
                    }
                    self.adjust_treasury(&symbol, -(*amount as i128))?;
                    let to_account = self.get_or_create_account(to);
                    Self::adjust_custom_balance(to_account, &symbol, *amount as i128)?;
                }
            }
        }
        self.charge_gas(tx)?;
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
    pub fn register_node(&mut self, mut node: Node) -> Result<Node, LedgerError> {
        if node.role == NodeRole::Validator {
            let stake_balance = {
                let account = self.get_account(&node.owner)?;
                account.stake_balance
            };
            if stake_balance < MIN_VALIDATOR_STAKE {
                return Err(LedgerError::Forbidden(format!(
                    "validator nodes require at least {} AIA staked",
                    MIN_VALIDATOR_STAKE
                )));
            }
        }
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
        node.scheduler_jobs_executed = 0;
        node.assignment_jobs_generated = 0;
        node.total_rewards = 0;
        node.last_reward_at = 0;
        self.state.nodes.insert(node.id.clone(), node.clone());
        self.recalculate_average_tasks();
        Ok(node)
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
            assignment_node_id: None,
            scheduler_node_id: None,
        };
        let assignment_node = self.select_assignment_node(task.preferred_region.as_deref());
        if let Some(ref assignment_id) = assignment_node {
            if let Some(node) = self.state.nodes.get_mut(assignment_id) {
                node.assignment_jobs_generated = node.assignment_jobs_generated.saturating_add(1);
            }
        }
        let scheduler_node = self.select_scheduler_node(task.preferred_region.as_deref());
        if let Some(ref scheduler_id) = scheduler_node {
            if let Some(node) = self.state.nodes.get_mut(scheduler_id) {
                node.scheduler_jobs_executed = node.scheduler_jobs_executed.saturating_add(1);
            }
        }
        task.assignment_node_id = assignment_node;
        task.scheduler_node_id = scheduler_node;
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
            let kind = TransactionKind::Payout {
                to: account.id.clone(),
                token: TokenKind::AIA,
                amount: share,
                task_id: "block_reward".into(),
            };
            let tx = build_transaction(
                format!("block-reward-{}-{}", now, account.id),
                now,
                kind,
                Some(account.id.clone()),
                MIN_GAS_PRICE,
                None,
            );
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
        let kind = TransactionKind::ChatResult {
            task_id: task_id.to_string(),
            aggregate,
            contributors,
        };
        Some(build_transaction(
            format!("chat-result-{}-{}", task_id, timestamp),
            timestamp,
            kind,
            Some(task.owner.clone()),
            MIN_GAS_PRICE,
            None,
        ))
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
                    self.apply_task_share_for_node(node_id, reward);
                }
                let provider_owner = self
                    .state
                    .providers
                    .get(&best.provider_id)
                    .map(|p| p.owner.clone())
                    .unwrap_or_else(|| best.provider_id.clone());
                let receipt_kind = TransactionKind::SegmentReceipt {
                    task_id: task_id.clone(),
                    segment_id: best.segment_id.clone(),
                    provider_id: best.provider_id.clone(),
                    tokens: best.tokens_processed,
                    latency_ms: best.latency_ms,
                    reward,
                };
                let receipt = build_transaction(
                    format!("segment-{}-{}", task_id, best.segment_id),
                    current_timestamp(),
                    receipt_kind,
                    Some(provider_owner.clone()),
                    MIN_GAS_PRICE,
                    None,
                );
                outputs.push(receipt);
                let payout_kind = TransactionKind::Payout {
                    to: best.provider_id.clone(),
                    token: TokenKind::AIA,
                    amount: reward,
                    task_id: task_id.clone(),
                };
                let payout = build_transaction(
                    format!("payout-{}-{}", task_id, best.provider_id),
                    current_timestamp(),
                    payout_kind,
                    Some(provider_owner),
                    MIN_GAS_PRICE,
                    None,
                );
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
        for node in self
            .state
            .nodes
            .values()
            .filter(|n| n.online && n.role == NodeRole::Validator)
        {
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
    pub treasury_gas_collected: u64,
    pub active_providers: usize,
    pub active_nodes: usize,
    pub validators_online: usize,
    pub scheduler_nodes_online: usize,
    pub assignment_nodes_online: usize,
    pub compute_nodes_online: usize,
    pub total_tasks: usize,
    pub consensus_round: u64,
    pub target_tokens_per_sec: u64,
    pub peak_tokens_per_sec: u64,
    pub total_tokens_processed: u128,
    pub max_parallel_nodes: u64,
    pub total_task_slots: u128,
    pub average_tasks_per_node: f64,
    pub total_contracts: usize,
    pub total_custom_tokens: usize,
    pub platform_token_count: usize,
    pub contract_events: usize,
    pub min_validator_stake: u64,
}

impl From<&LedgerState> for NetworkSummary {
    fn from(state: &LedgerState) -> Self {
        let active_providers = state
            .providers
            .values()
            .filter(|provider| provider.last_heartbeat.is_some())
            .count();
        let mut validators_online = 0;
        let mut scheduler_nodes_online = 0;
        let mut assignment_nodes_online = 0;
        let mut compute_nodes_online = 0;
        for node in state.nodes.values().filter(|node| node.online) {
            match node.role {
                NodeRole::Validator => validators_online += 1,
                NodeRole::Scheduler => scheduler_nodes_online += 1,
                NodeRole::Assignment => assignment_nodes_online += 1,
                NodeRole::Compute => compute_nodes_online += 1,
            }
        }
        let active_nodes = validators_online
            + scheduler_nodes_online
            + assignment_nodes_online
            + compute_nodes_online;
        Self {
            block_height: state.blocks.len() as u64,
            treasury_aia: state.treasury.aia_balance,
            treasury_gas_collected: state.treasury.gas_collected,
            active_providers,
            active_nodes,
            validators_online,
            scheduler_nodes_online,
            assignment_nodes_online,
            compute_nodes_online,
            total_tasks: state.tasks.len(),
            consensus_round: state.network_capacity.consensus_round,
            target_tokens_per_sec: state.network_capacity.target_tokens_per_sec,
            peak_tokens_per_sec: state.network_capacity.peak_observed_tokens_per_sec,
            total_tokens_processed: state.network_capacity.tokens_processed,
            max_parallel_nodes: state.network_capacity.max_parallel_nodes,
            total_task_slots: state.network_capacity.total_task_slots,
            average_tasks_per_node: state.network_capacity.average_tasks_per_node,
            total_contracts: state.contracts.len(),
            total_custom_tokens: state.token_definitions.len(),
            platform_token_count: state.platform_tokens.len(),
            contract_events: state.contract_events.len(),
            min_validator_stake: MIN_VALIDATOR_STAKE,
        }
    }
}
