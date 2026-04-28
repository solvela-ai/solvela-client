use std::io::{BufRead, Write};

use solvela_client::{ClientBuilder, SolvelaClient, Wallet};
use solvela_client_cli_args::{load_wallet, save_wallet, GatewayArgs, RpcArgs, WalletArgs};

use crate::WalletAction;

pub async fn run(
    action: WalletAction,
    wallet_args: &WalletArgs,
    gateway_args: &GatewayArgs,
    rpc_args: &RpcArgs,
) -> Result<(), String> {
    match action {
        WalletAction::Create { force } => create(wallet_args, force),
        WalletAction::Import {
            mnemonic,
            keypair,
            force,
        } => {
            if mnemonic {
                import_mnemonic(wallet_args, force)
            } else if keypair {
                import_keypair(wallet_args, force)
            } else {
                Err("specify --mnemonic or --keypair".to_string())
            }
        }
        WalletAction::Balance => balance(wallet_args, gateway_args, rpc_args).await,
        WalletAction::Address => address(wallet_args),
        WalletAction::Export { yes } => export(wallet_args, yes),
    }
}

fn create(wallet_args: &WalletArgs, force: bool) -> Result<(), String> {
    let (wallet, mnemonic) = Wallet::create();
    let bytes = wallet.to_keypair_bytes();
    let path = save_wallet(&wallet_args.wallet_file, &bytes, force)?;

    println!("===");
    println!("{}", wallet.address());
    println!("===");
    println!();
    println!("Wallet saved to: {}", path.display());
    println!();
    println!("=== BACKUP YOUR SEED PHRASE ===");
    println!("{mnemonic}");
    println!("=== END SEED PHRASE ===");

    Ok(())
}

fn import_mnemonic(wallet_args: &WalletArgs, force: bool) -> Result<(), String> {
    eprint!("Enter seed phrase: ");
    std::io::stderr()
        .flush()
        .map_err(|e| format!("failed to flush stderr: {e}"))?;

    let mut phrase = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut phrase)
        .map_err(|e| format!("failed to read stdin: {e}"))?;

    let phrase = phrase.trim();
    if phrase.is_empty() {
        return Err("empty seed phrase".to_string());
    }

    let wallet = Wallet::from_mnemonic(phrase).map_err(|e| format!("invalid mnemonic: {e}"))?;
    let bytes = wallet.to_keypair_bytes();
    let path = save_wallet(&wallet_args.wallet_file, &bytes, force)?;

    println!("Imported address: {}", wallet.address());
    println!("Wallet saved to: {}", path.display());

    Ok(())
}

fn import_keypair(wallet_args: &WalletArgs, force: bool) -> Result<(), String> {
    eprint!("Enter base58 keypair: ");
    std::io::stderr()
        .flush()
        .map_err(|e| format!("failed to flush stderr: {e}"))?;

    let mut b58 = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut b58)
        .map_err(|e| format!("failed to read stdin: {e}"))?;

    let b58 = b58.trim();
    if b58.is_empty() {
        return Err("empty keypair input".to_string());
    }

    let wallet = Wallet::from_keypair_b58(b58).map_err(|e| format!("invalid keypair: {e}"))?;
    let bytes = wallet.to_keypair_bytes();
    let path = save_wallet(&wallet_args.wallet_file, &bytes, force)?;

    println!("Imported address: {}", wallet.address());
    println!("Wallet saved to: {}", path.display());

    Ok(())
}

async fn balance(
    wallet_args: &WalletArgs,
    gateway_args: &GatewayArgs,
    rpc_args: &RpcArgs,
) -> Result<(), String> {
    let wallet = load_wallet(wallet_args)?;
    let config = ClientBuilder::new()
        .gateway_url(&gateway_args.gateway)
        .rpc_url(&rpc_args.rpc_url)
        .build_config();

    let client =
        SolvelaClient::new(wallet, config).map_err(|e| format!("failed to create client: {e}"))?;

    let balance = client
        .usdc_balance()
        .await
        .map_err(|e| format!("failed to fetch balance: {e}"))?;

    println!("{balance:.6} USDC");

    Ok(())
}

fn address(wallet_args: &WalletArgs) -> Result<(), String> {
    let wallet = load_wallet(wallet_args)?;
    println!("{}", wallet.address());
    Ok(())
}

fn export(wallet_args: &WalletArgs, yes: bool) -> Result<(), String> {
    let wallet = load_wallet(wallet_args)?;

    if !yes {
        eprint!("WARNING: This will print your private key. Continue? [y/N] ");
        std::io::stderr()
            .flush()
            .map_err(|e| format!("failed to flush stderr: {e}"))?;

        let mut answer = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut answer)
            .map_err(|e| format!("failed to read stdin: {e}"))?;

        let answer = answer.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            return Err("aborted".to_string());
        }
    }

    println!("{}", wallet.to_keypair_b58());

    Ok(())
}
