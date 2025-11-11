//! GPANG Network ledger module.
//!
//! The ledger owns all on-chain state including accounts, GPU providers, nodes,
//! and task execution records. State is persisted as JSON on disk so that the
//! entire blockchain can operate without an external database. All mutating
//! operations funnel through [`Ledger::apply_transaction`] to provide a single
//! entrypoint for token and reward accounting.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;
use tracing::info;

pub mod types;

use types::{
    Account, Block, LedgerState, ModelProfile, Node, NodeRevenue, Provider, Task, TaskSegment,
    TokenKind, Transaction, TransactionKind,
};

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
                let reward_after_fees = segment.reward;
                task.segments.push(segment);
                if let Some(node) = self.state.nodes.get_mut(provider_id) {
                    node.total_segments = node.total_segments.saturating_add(1);
                    node.total_tokens_processed =
                        node.total_tokens_processed.saturating_add(*tokens);
                    node.total_rewards_aia =
                        node.total_rewards_aia.saturating_add(reward_after_fees);
                    node.total_latency_ms =
                        node.total_latency_ms.saturating_add((*latency_ms).into());
                }
                if task.segments.len() as u64 * 100_000 >= task.total_tokens {
                    task.completed = true;
                    if let Some(best) = task.segments.iter().min_by_key(|seg| seg.latency_ms) {
                        task.winning_provider = Some(best.provider_id.clone());
                    }
                }
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
            timestamp: Utc::now(),
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
    pub fn upsert_provider(&mut self, provider: Provider) {
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
        Ok(())
    }

    /// Registers a node in the ledger.
    pub fn register_node(&mut self, mut node: Node) {
        if let Some(existing) = self.state.nodes.get(&node.id) {
            node.total_segments = existing.total_segments;
            node.total_tokens_processed = existing.total_tokens_processed;
            node.total_rewards_aia = existing.total_rewards_aia;
            node.total_latency_ms = existing.total_latency_ms;
        }
        node.registered_at = Utc::now();
        self.state.nodes.insert(node.id.clone(), node);
    }

    /// Submits a new task and returns the identifier.
    pub fn submit_task(
        &mut self,
        owner: String,
        content_hash: String,
        total_tokens: u64,
        target_profile: ModelProfile,
        providers: Vec<String>,
    ) -> String {
        let id = format!("task-{}", self.state.next_task_id);
        self.state.next_task_id += 1;
        let task = Task {
            id: id.clone(),
            owner,
            content_hash,
            total_tokens,
            target_profile,
            requested_providers: providers,
            segments: Vec::new(),
            created_at: Utc::now(),
            completed: false,
            winning_provider: None,
        };
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
        let now = Utc::now();
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
                id: format!("block-reward-{}-{}", now.timestamp(), account.id),
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
        provider.last_heartbeat = Some(Utc::now());
        Ok(())
    }

    /// Updates a node's online status.
    pub fn update_node_status(&mut self, node_id: &str, online: bool) -> Result<(), LedgerError> {
        let node = self
            .state
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| LedgerError::NodeNotFound(node_id.to_string()))?;
        node.online = online;
        Ok(())
    }

    /// Returns aggregate revenue statistics for a node.
    pub fn node_revenue(&self, node_id: &str) -> Result<NodeRevenue, LedgerError> {
        let node = self
            .state
            .nodes
            .get(node_id)
            .ok_or_else(|| LedgerError::NodeNotFound(node_id.to_string()))?;
        let average_latency_ms = if node.total_segments > 0 {
            Some(node.total_latency_ms as f64 / node.total_segments as f64)
        } else {
            None
        };
        Ok(NodeRevenue {
            node_id: node.id.clone(),
            owner: node.owner.clone(),
            total_segments: node.total_segments,
            total_tokens_processed: node.total_tokens_processed,
            total_rewards_aia: node.total_rewards_aia,
            average_latency_ms,
        })
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
        }
    }
}
