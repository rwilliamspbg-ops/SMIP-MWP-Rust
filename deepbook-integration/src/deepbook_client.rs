//! DeepBook v3 Client wrapper for Predict markets
//! 
//! Integrates with MystenLabs/deepbookv3 packages/predict

use super::DeepBookEvent;

/// DeepBook client interface for agent operations
pub trait DeepBookClient {
    /// Create BalanceManager for SUI balance operations
    fn create_balance_manager(&self) -> Result<BalanceManager, ClientError>;
    
    /// Deposit to vault (liquidity provision flow)
    fn deposit_to_vault(
        &self,
        amount: u128,
        lp_token_receiver: String,
    ) -> Result<VaultReceipt, ClientError>;
    
    /// Place binary order on Predict market
    fn place_order(
        &self,
        market_id: String,
        position_type: PositionType,
        qty: u64,
        price: u128,
        leverage: Option<u8>,
    ) -> Result<OrderReceipt, ClientError>;
    
    /// Mint position from filled order
    fn mint_position(&self, order_id: &str) -> Result<MintReceipt, ClientError>;
    
    /// Read oracle price for a specific market
    fn read_oracle_price(&self, market_id: &str) -> Result<OraclePrice, ClientError>;
}

/// Balance manager for SUI operations
pub struct BalanceManager {
    pub sui_balance: u128,
    pub position_count: u64,
}

impl BalanceManager {
    pub fn new(initial_balance: u128) -> Self {
        Self {
            sui_balance: initial_balance,
            position_count: 0,
        }
    }
    
    pub fn deposit(&mut self, amount: u128) {
        self.sui_balance += amount;
    }
}

/// Vault deposit receipt (liquidity provision proof)
#[derive(Debug, Clone)]
pub struct VaultReceipt {
    pub lp_token_id: String,
    pub deposit_amount: u128,
    pub timestamp: u64,
}

/// Order placement receipt
#[derive(Debug, Clone)]
pub struct OrderReceipt {
    pub order_id: String,
    pub market_id: String,
    pub position_type: PositionType,
    pub qty: u64,
    pub price: u128,
    pub leverage: Option<u8>,
}

/// Mint receipt for position creation
#[derive(Debug, Clone)]
pub struct MintReceipt {
    pub position_id: String,
    pub order_id: String,
    pub fill_price: u128,
    pub filled_qty: u64,
}

/// Oracle price response
#[derive(Debug, Clone)]
pub struct OraclePrice {
    pub market_id: String,
    pub current_price: u128,
    pub oracle_source: String,
    pub timestamp: u64,
}

/// Client errors
#[derive(Debug, Clone)]
pub enum ClientError {
    InvalidAddress(String),
    InsufficientBalance(u128, u128),
    OrderExpired(OrderReceipt),
    SettlementFailed(MintReceipt),
    OracleTimeout(String),
    DeepBookApiError(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ClientError::InvalidAddress(addr) => write!(f, "Invalid address: {}", addr),
            ClientError::InsufficientBalance(required, available) => {
                write!(f, "Insufficient balance: need {}, have {}", required, available)
            }
            ClientError::OrderExpired(order) => {
                write!(f, "Order expired: {:?}", order)
            }
            ClientError::SettlementFailed(receipt) => {
                write!(f, "Settlement failed for position {:?}", receipt)
            }
            ClientError::OracleTimeout(source) => {
                write!(f, "Oracle timeout from {}", source)
            }
            ClientError::DeepBookApiError(msg) => {
                write!(f, "DeepBook API error: {}", msg)
            }
        }
    }
}
