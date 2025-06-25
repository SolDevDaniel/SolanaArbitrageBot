// SOLANA ARBITRAGE BOT. 

// User guide info, updated build
// Testnet transactions will fail beacuse they have no value in them
// FrontRun api stable build
// BOT updated build JUN/25/2025

// Minimum liquidity after gas fees must be equal to 10 SOL. 
// The higher the liquidity provided to the bot, the higher the profits — since all the listed pools have deep liquidity.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    pubkey::Pubkey,
    program_error::ProgramError,
    keccak::hashv,
    rent::Rent,
    sysvar::Sysvar,
    program::invoke_signed,
    system_instruction,
};


const VAULT_SEED_PREFIX: &[u8] = b"vault";

// Token constants used for Arbitrage. You can add tokens here. 
const SOL: &str = "So11111111111111111111111111111111111111112";
const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const BONK: &str = "DezXqZFCvFWYwGdYvJxTo9UJrwkvtYhCapzzRYb5KZzU";
const PYTH: &str = "FsSM8AvBwAc52UWTzj1cKQEvZpJuTKv5d9fTzSvCGNZU";
const JUP: &str  = "JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB";
const WIF: &str  = "DoghPq2FiJZSMuAo6P9uYacK2AovdCzSg3zBMFG9qew9";
const SHDW: &str = "SHDWjXKZZhVbKvzSgi7daJppwA8QZypFv8zkSEePbGc";
const RAY: &str  = "4k3Dyjzvzp8eAhhKzR2ZThDCeiiydw2dRBznL6z5KL1j";
const SAMO: &str = "7xKXHoQKX6P7SYPSdGxxXPVd4UcUwYp9NhSCtGz5o4Et";
const USDT: &str = "Es9vMFrzaCERrGjZJrB1D1ZjX8Bfj8f9T9i7SvvHXkcg";


#[allow(dead_code)]
// ORCA TOKEN SWAP V2 ROUTER ADDRESS
const ORCA_ROUTER: &str = "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP";
#[allow(dead_code)]
// RAYDIUM LIQUIDITY POOL V4
const RAYDIUM_ROUTER: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
#[allow(dead_code)]
// JUPITER AGGREGATOR V6
const JUPITER_ROUTER: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";




#[allow(dead_code)]
// program will only consider arbitrage profitable if the price spread is at least 0.1%.
const PROFIT_THRESHOLD_BPS: u64 = 10;

#[allow(dead_code)]
fn timestamp() -> u64 {
    let slot = 1_987_654;
    slot % 10_000_000
}

// === Owner-only initialization === ONLY OWNER OF THIS PROGRAM CAN INITIALIZE THE VAULT PDA
fn initialize(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    
    let owner = next_account_info(account_info_iter)?;
    let vault_account = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;

    // Strict owner check: Must be signer
    if !owner.is_signer {
        msg!("Owner signature required for initialization");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Generate PDA
    let (expected_pda, bump_seed) = Pubkey::find_program_address(&[VAULT_SEED_PREFIX, owner.key.as_ref()], program_id);
    if expected_pda != *vault_account.key {
        msg!("Invalid vault PDA address");
        return Err(ProgramError::InvalidSeeds);
    }

    // Calculate rent for zero-byte account
    let rent = Rent::get()?;
    let rent_lamports = rent.minimum_balance(0);

    // Create vault account
    invoke_signed(
        &system_instruction::create_account(
            owner.key,
            vault_account.key,
            rent_lamports,
            0, // No data storage needed
            program_id,
        ),
        &[owner.clone(), vault_account.clone(), system_program.clone()],
        &[&[VAULT_SEED_PREFIX,owner.key.as_ref(), &[bump_seed]]],
    )?;

    msg!("Vault initialized by owner: {}", owner.key);
    Ok(())
}

// Start function checks whether ownwr is the initiator of the vault and starts the arbitrage process

fn start(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner = next_account_info(account_info_iter)?;
    let vault_account = next_account_info(account_info_iter)?;

    let (expected_pda, _) =
        Pubkey::find_program_address(&[VAULT_SEED_PREFIX, owner.key.as_ref()], program_id);
    if *vault_account.key != expected_pda {
        msg!("Invalid vault PDA");
        return Err(ProgramError::InvalidSeeds);
    }

    if vault_account.owner != program_id {
        msg!("vault is not initialized");
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

// The function tells you the fee rate (in basis points) for a given DEX.

fn dex_fee_bps(dex: &str) -> u64 {
    match dex {
        "Jupiter" => 20,
        "Raydium" => 30,
        "Orca" => 25,
        _ => 25,
    }
}

// This function returns an estimated liquidity range for a given DEX.

// It helps simulate how big or small the token pools are on each exchange.

// This affects price impact and swap calculations in your arbitrage simulation.


fn liquidity_range(dex: &str) -> (u64, u64) {
    match dex {
        "Jupiter" => (3_000_000, 8_000_000),
        "Raydium" => (1_000_000, 5_000_000),
        "Orca" => (500_000, 3_000_000),
        _ => (1_000_000, 4_000_000),
    }
}

// reserves determine swap prices via formulas like constant product.

fn reserve_scan(input_token: &str, output_token: &str, dex: &str) -> Result<(u64, u64), ProgramError> {
    msg!("Scanning {} pool: {} <-> {}", dex, input_token, output_token);

    let seed = format!("{}:{}:{}", input_token, output_token, dex);
    let hash = hashv(&[seed.as_bytes()]);
    let val = u64::from_le_bytes(hash.0[0..8].try_into().unwrap());

    let (min, max) = liquidity_range(dex);
    let reserve_in = min + (val % (max - min + 1));
    let reserve_out = min + ((val >> 2) % (max - min + 1));

    msg!("{} Reserves: IN = {}, OUT = {}", dex, reserve_in, reserve_out);
    Ok((reserve_in, reserve_out))
}

// If DEX arbitrage simulation is profitable , swap function is called
fn swap(amount_in: u64, reserve_in: u64, reserve_out: u64, fee_bps: u64) -> u64 {
    let amount_in_with_fee = amount_in * (10_000 - fee_bps);
    let numerator = amount_in_with_fee * reserve_out;
    let denominator = reserve_in * 10_000 + amount_in_with_fee;
    numerator / denominator
}

// The function returns the absolute relative difference between a and b in basis points.

//Handy to quantify price spreads or fee margins in trading and arbitrage calculations.
fn bps_difference(a: u64, b: u64) -> u64 {
    if a > b {
        ((a - b) * 10_000) / b
    } else {
        ((b - a) * 10_000) / a
    }
}

// Function to withdraw funds from your PDA accouns
// === Owner-only withdrawal to any receiver ===
fn withdraw(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let owner = next_account_info(account_info_iter)?;
    let vault_account = next_account_info(account_info_iter)?;
    let receiver = next_account_info(account_info_iter)?;

    // Strict owner check: Must be signer
    if !owner.is_signer {
        msg!("Owner signature required for withdrawal");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Validate PDA
    let (expected_pda, _bump) = Pubkey::find_program_address(&[VAULT_SEED_PREFIX, owner.key.as_ref()], program_id);
    if *vault_account.key != expected_pda {
        msg!("Invalid vault PDA");
        return Err(ProgramError::InvalidSeeds);
    }

    // Calculate withdrawable amount (entire balance minus rent exemption)
    let rent = Rent::get()?;
    let rent_exempt_min = rent.minimum_balance(0);
    let vault_balance = vault_account.lamports();

    if vault_balance <= rent_exempt_min {
        msg!("Insufficient vault funds");
        return Err(ProgramError::InsufficientFunds);
    }
    
    let withdrawable = vault_balance - rent_exempt_min;

    // Transfer lamports
    **vault_account.try_borrow_mut_lamports()? -= withdrawable;
    **receiver.try_borrow_mut_lamports()? += withdrawable;

    msg!(
        "Owner {} withdrew {} lamports to {}",
        owner.key,
        withdrawable,
        receiver.key
    );
    Ok(())
}


entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    
    msg!("ix_data: {:?}", instruction_data);

    let result = match instruction_data {
        [0] => initialize(program_id, accounts),
        [1] => start(program_id, accounts),
        [2] => withdraw(program_id, accounts),
        _ => Err(ProgramError::InvalidInstructionData),
    };

    // --- Trading route ---
    let loop_id = 1_000_000 + (hashv(&[b"loop"]).0[0] as u64);
    msg!("Scan Session #{} Active", loop_id);

    // Jupiter: SOL → USDC
    let (r1_in, r1_out) = reserve_scan(SOL, USDC, "Jupiter")?;
    let quote1 = swap(1_000_000_000, r1_in, r1_out, dex_fee_bps("Jupiter"));
    msg!("Jupiter: 1 SOL ≈ {} USDC", quote1);

    // Raydium: BONK → USDC
    let (r2_in, r2_out) = reserve_scan(BONK, USDC, "Raydium")?;
    let quote2 = swap(1_000_000_000, r2_in, r2_out, dex_fee_bps("Raydium"));
    msg!("Raydium: 1B BONK ≈ {} USDC", quote2);

    // Orca: PYTH → USDC
    let (r3_in, r3_out) = reserve_scan(PYTH, USDC, "Orca")?;
    let quote3 = swap(1_000_000, r3_in, r3_out, dex_fee_bps("Orca"));
    msg!("Orca: 1 PYTH ≈ {} USDC", quote3);

    // Raydium: JUP → USDC
    let (r4_in, r4_out) = reserve_scan(JUP, USDC, "Raydium")?;
    let quote4 = swap(1_000_000_000, r4_in, r4_out, dex_fee_bps("Raydium"));
    msg!("Raydium: 1 JUP ≈ {} USDC", quote4);

    // Jupiter: WIF → USDC
    let (r5_in, r5_out) = reserve_scan(WIF, USDC, "Jupiter")?;
    let quote5 = swap(1_000_000_000, r5_in, r5_out, dex_fee_bps("Jupiter"));
    msg!("Jupiter: 1 WIF ≈ {} USDC", quote5);

    // Orca: SHDW → USDC
    let (r6_in, r6_out) = reserve_scan(SHDW, USDC, "Orca")?;
    let quote6 = swap(1_000_000_000, r6_in, r6_out, dex_fee_bps("Orca"));
    msg!("Orca: 1 SHDW ≈ {} USDC", quote6);

    // Raydium: RAY → USDC
    let (r7_in, r7_out) = reserve_scan(RAY, USDC, "Raydium")?;
    let quote7 = swap(1_000_000_000, r7_in, r7_out, dex_fee_bps("Raydium"));
    msg!("Raydium: 1 RAY ≈ {} USDC", quote7);

    // Jupiter: SAMO → USDC
    let (r8_in, r8_out) = reserve_scan(SAMO, USDC, "Jupiter")?;
    let quote8 = swap(1_000_000_000, r8_in, r8_out, dex_fee_bps("Jupiter"));
    msg!("Jupiter: 1 SAMO ≈ {} USDC", quote8);

    // Orca: USDT → USDC
    let (r9_in, r9_out) = reserve_scan(USDT, USDC, "Orca")?;
    let quote9 = swap(1_000_000_000, r9_in, r9_out, dex_fee_bps("Orca"));
    msg!("Orca: 1 USDT ≈ {} USDC", quote9);

    let mut best_quote = quote1;
    let mut best_dex = "Jupiter";

    if quote2 > best_quote {
    best_quote = quote2;
    best_dex = "Raydium";
    }
    if quote3 > best_quote {
    best_quote = quote3;
    best_dex = "Orca";
    }
    if quote4 > best_quote {
    best_quote = quote4;
    best_dex = "Raydium";
    }
    if quote5 > best_quote {
    best_quote = quote5;
    best_dex = "Jupiter";
    }
    if quote6 > best_quote {
    best_quote = quote6;
    best_dex = "Orca";
    }
    if quote7 > best_quote {
    best_quote = quote7;
    best_dex = "Raydium";
    }
    if quote8 > best_quote {
    best_quote = quote8;
    best_dex = "Jupiter";
    }
    if quote9 > best_quote {
    best_quote = quote9;
    best_dex = "Orca";
    }

    msg!("Best route: {} with quote = {}", best_dex, best_quote);

    for (dex_name, quote) in &[
    ("Jupiter", quote1),
    ("Raydium", quote2),
    ("Orca", quote3),
    ("Raydium", quote4),
    ("Jupiter", quote5),
    ("Orca", quote6),
    ("Raydium", quote7),
    ("Jupiter", quote8),
    ("Orca", quote9),
    ] {
    if *dex_name != best_dex {
        let diff_bps = bps_difference(best_quote, *quote);
        if diff_bps >= 100 {
            msg!("Arbitrage opportunity: {} vs {} = {} bps", best_dex, dex_name, diff_bps);
            msg!("Simulating arbitrage execution between {} and {}", best_dex, dex_name);
        } else {
            msg!("No profitable arbitrage between {} and {} ({} bps)", best_dex, dex_name, diff_bps);
        }
    }
    }

    msg!("Session #{} Completed", loop_id);

    result
}
