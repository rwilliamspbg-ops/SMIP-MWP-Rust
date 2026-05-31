//! DeepBook v3 Agent Library for Sui Overflow 2026 Hackathon
//! 
//! Core functionality:
//! - Read oracle prices → place binary positions on Predict
//! - Liquidity provision flow (supply to vault)
//! - Dashboard showing agent-driven volume on DeepBook pool

pub mod deepbook_client;
pub mod oracle_reader;
pub mod position_manager;
pub mod vault_flow;

use serde::{Deserialize, Serialize};

/// DeepBook event structure for zero-copy parsing
#[derive(Debug, Clone, Deserialize)]
pub struct DeepBookEvent {
    pub event: String,
    pub pool_id: String,
    pub order_type: OrderType,
    pub maker_address: String,
    pub taker_address: OptionalString,
    pub fill_price: u128,
    pub filled_qty: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Order {
    pub order_id: String,
    pub asset_class: AssetClass,
    pub order_type: OrderType,
    pub market: Market,
    pub maker_address: String,
    pub qty: u64,
    pub price: u128,
}

#[derive(Debug, Clone, Deserialize)]
pub enum OrderType {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Deserialize)]
pub enum AssetClass {
    Sui,
    DeepBook,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Market {
    pub pool_id: String,
    pub asset_a_class: String,
    pub asset_b_class: String,
}

/// Agent state for settlement correctness tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub position_count: u64,
    pub total_volume: u128,
    pub filled_positions: Vec<Position>,
    pub pending_orders: Vec<Order>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub order_id: String,
    pub fill_price: u128,
    pub filled_qty: u64,
    pub status: PositionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PositionStatus {
    Open,
    Filled,
    Settled,
}
