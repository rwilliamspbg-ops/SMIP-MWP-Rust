//! DeepBook v3 Agent - Sui Overflow 2026 Hackathon MVP
//! 
//! Core flows:
//! 1. Read oracle prices → place binary positions on Predict
//! 2. Liquidity provision flow (supply to vault)
//! 3. Dashboard showing agent-driven volume on DeepBook pool

use deepbook_agent::{
    deepbook_client::DeepBookClient,
    oracle_reader::OracleReader,
    position_manager::PositionManager,
    vault_flow::VaultFlow,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== DeepBook v3 Agent for Sui Overflow 2026 Hackathon ===\n");
    
    // Initialize components
    let oracle_reader = OracleReader::new(
        super::oracle_reader::OracleSource::DeepBookNative,
        std::time::Duration::from_secs(60),
    );
    
    let position_manager = PositionManager::default();
    let vault_flow = VaultFlow::default();
    
    // Demo: Simulate agent decision flow
    println!("📊 Oracle Price Read (Market ID: prediction_token)");
    let price_oracle = oracle_reader.read_price("prediction_token").await?;
    println!("  Current Price: {} microSUI", price_oracle.current_price);
    println!("  Oracle Source: {}", price_oracle.oracle_source);\n    
    println!("\n🎯 Agent Decision: Place Binary Position");
    let market_id = "prediction_token_yes".to_string();
    let receipt = position_manager.place_binary_order(
        market_id.clone(),
        super::position_manager::BinaryOutcome::Yes,
        1_000_000u64, // 1 million base units
        price_oracle.current_price,
        Some(5), // 5x leverage
    ).await?;
    println!("  Order ID: {}", receipt.order_id);
    println!("  Leverage: {}x", receipt.leverage.unwrap_or(1));
    
    println!("\n💰 Liquidity Provision Flow");
    let deposit_amount = 1_000_000_000u128; // $1M in microSUI
    let vault_receipt = vault_flow.supply_to_vault(deposit_amount).await?;
    println!("  LP Token ID: {}", vault_receipt.lp_token_id);
    println!("  Deposit Amount: {} microSUI", vault_receipt.deposit_amount);\n    
    println!("\n✅ MVP Complete!");
    println!("Next Steps:");
    println!("1. Test on Sui testnet with actual transactions");
    println!("2. Integrate formal proofs for settlement correctness");
    println!("3. Build dashboard showing agent-driven volume");
    
    Ok(())
}
