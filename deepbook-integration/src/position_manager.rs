//! Position Manager for Binary Prediction Markets
//! 
//! Manages binary (yes/no) positions with leverage support

use super::{OrderReceipt, MintReceipt, ClientError, BinaryOutcome};

/// Binary position type (YES/NO outcomes)
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOutcome {
    Yes,
    No,
}

impl std::fmt::Display for BinaryOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BinaryOutcome::Yes => write!(f, "YES"),
            BinaryOutcome::No => write!(f, "NO"),
        }
    }
}

/// Position manager for binary markets
pub struct PositionManager {
    pub positions: Vec<BinaryPosition>,
    pub total_leverage_cap: u64, // In base units (e.g., 10x = 10_000_000_000)
}

impl Default for PositionManager {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            total_leverage_cap: 10_000_000_000u64, // Default 10x leverage
        }
    }
}

impl PositionManager {
    /// Create new position manager with custom leverage cap
    pub fn new(leverage_cap: u64) -> Self {
        Self {
            positions: Vec::new(),
            total_leverage_cap: leverage_cap,
        }
    }
    
    /// Place binary order (YES/NO outcome)
    pub async fn place_binary_order(
        &mut self,
        market_id: String,
        outcome: BinaryOutcome,
        qty: u64,
        price: u128,
        leverage: Option<u8>,
    ) -> Result<OrderReceipt, ClientError> {
        let leverage = leverage.unwrap_or(1); // Default 1x
        
        if leverage > 10 {
            return Err(ClientError::InvalidAddress("Max leverage: 10x".into()));
        }
        
        let order_id = format!(
            "ord_{}_{}_{}",
            market_id,
            outcome.to_string().to_uppercase(),
            qty
        );
        
        let receipt = OrderReceipt {
            order_id: order_id.clone(),
            market_id,
            position_type: outcome.clone(),
            qty,
            price,
            leverage: Some(leverage as u8),
        };
        
        self.positions.push(BinaryPosition {
            order_id: order_id.clone(),
            outcome,
            qty,
            price,
            leverage,
            status: PositionStatus::Pending,
            timestamp: 0u64, // Would be actual timestamp in production
        });
        
        Ok(receipt)
    }
    
    /// Mint position from filled order (settlement proof)
    pub async fn mint_position(
        &mut self,
        order_id: &str,
        fill_price: u128,
        filled_qty: u64,
    ) -> Result<MintReceipt, ClientError> {
        // Find matching pending position
        let position_idx = self
            .positions
            .iter()
            .position(|p| p.order_id == order_id.to_string())
            .ok_or(ClientError::SettlementFailed(MintReceipt {
                position_id: order_id.to_string(),
                order_id: order_id.to_string(),
                fill_price: 0u128,
                filled_qty: 0u64,
            }))?;
        
        let position = &mut self.positions[position_idx];
        position.status = PositionStatus::Filled;
        position.price = fill_price;
        position.qty = filled_qty;
        
        // Formal proof: settlement invariant - quantity must be > 0
        if filled_qty == 0 {
            return Err(ClientError::SettlementFailed(MintReceipt {
                position_id: order_id.to_string(),
                order_id: order_id.to_string(),
                fill_price: fill_price,
                filled_qty: 0u64, // Zero quantity = invalid settlement
            }));
        }
        
        Ok(MintReceipt {
            position_id: order_id.to_string(),
            order_id: order_id.to_string(),
            fill_price,
            filled_qty,
        })
    }
    
    /// Close all positions (settlement)
    pub async fn close_all_positions(&mut self) -> Result<Vec<MintReceipt>, ClientError> {
        let mut receipts = Vec::new();
        
        for position in &mut self.positions {
            if matches!(position.status, PositionStatus::Filled) {
                let receipt = MintReceipt {
                    position_id: position.order_id.clone(),
                    order_id: position.order_id.clone(),
                    fill_price: position.price,
                    filled_qty: position.qty,
                };
                receipts.push(receipt);
            } else {
                // Cancel pending position
                println!("Canceling pending position: {}", position.order_id);
            }
        }
        
        self.positions.retain(|p| matches!(p.status, PositionStatus::Pending));
        
        Ok(receipts)
    }
    
    /// Get total exposure across all positions
    pub fn total_exposure(&self) -> u128 {
        self.positions
            .iter()
            .filter(|p| matches!(p.status, PositionStatus::Filled))
            .map(|p| p.qty as u128 * p.price)
            .sum()
    }
    
    /// Get position by order ID
    pub fn get_position(&self, order_id: &str) -> Option<&BinaryPosition> {
        self.positions.iter().find(|p| p.order_id == order_id.to_string())
    }
}

/// Binary position structure
#[derive(Debug, Clone)]
pub struct BinaryPosition {
    pub order_id: String,
    pub outcome: BinaryOutcome,
    pub qty: u64,
    pub price: u128,
    pub leverage: u8,
    pub status: PositionStatus,
    pub timestamp: u64,
}

/// Position status tracking
#[derive(Debug, Clone, PartialEq)]
pub enum PositionStatus {
    Pending,      // Order placed, awaiting fill
    Filled,       // Order filled, position active
    Settled,      // Position closed/settled
    Cancelled,    // Order cancelled
}

impl std::fmt::Display for PositionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PositionStatus::Pending => write!(f, "PENDING"),
            PositionStatus::Filled => write!(f, "FILLED"),
            PositionStatus::Settled => write!(f, "SETTLED"),
            PositionStatus::Cancelled => write!(f, "CANCELLED"),
        }
    }
}
