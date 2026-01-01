// GRPC клиент для подключения к Yellowstone Geyser

use yellowstone_grpc_client::{GeyserGrpcClient, ClientTlsConfig};
use yellowstone_grpc_proto::prelude::*;
use bs58;
use tracing::{info, error};
use std::time::Duration;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::error::Error;

use crate::AppState;
use crate::CreateTransaction;
use std::sync::Arc;

pub async fn run_grpc_subscription(state: Arc<AppState>) -> anyhow::Result<()> {
    let endpoint = "https://fr.grpc.gadflynode.com:25565";
    let token = ""; // Gadflynode public endpoint doesn't require token

    let mut backoff_interval = Duration::from_secs(1);
    loop {
        match subscribe_once(endpoint, token, state.clone()).await {
            Ok(_) => {
                // Успешное подключение - сбрасываем backoff
                backoff_interval = Duration::from_secs(1);
            }
            Err(e) => {
                error!("GRPC error: {} (will reconnect in {:?})", e, backoff_interval);
                tokio::time::sleep(backoff_interval).await;
                // Увеличиваем задержку экспоненциально, максимум 30 секунд
                backoff_interval = std::cmp::min(backoff_interval * 2, Duration::from_secs(30));
            }
        }
    }
}

async fn subscribe_once(endpoint: &str, token: &str, state: Arc<AppState>) -> anyhow::Result<()> {
    info!("Connecting to GRPC endpoint: {}", endpoint);

    // Создаем клиент
    let mut client = GeyserGrpcClient::build_from_shared(endpoint.to_string())?
        .x_token(if token.is_empty() { None } else { Some(token.to_string()) })?
        .tls_config(ClientTlsConfig::new().with_native_roots())?
        .connect()
        .await?;

    info!("✅ GRPC channel connected successfully");

    // Создаем filter для транзакций Pump.fun
    let pump_fun_program_id = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
    let mut transactions_filter = HashMap::new();
    transactions_filter.insert(
        "pump_fun".to_string(),
        SubscribeRequestFilterTransactions {
            account_include: vec![pump_fun_program_id.to_string()],
            vote: Some(false),
            failed: Some(false),
            signature: None,
            account_exclude: vec![],
            account_required: vec![],
        },
    );

    info!("Subscribing to Pump.fun Create via Yellowstone GRPC...");

    // Получаем stream и subscribe_tx
    let (mut subscribe_tx, mut updates_stream) = client.subscribe().await?;

    // Создаем subscription request
    let request = SubscribeRequest {
        transactions: transactions_filter,
        commitment: Some(CommitmentLevel::Processed as i32),
        ..Default::default()
    };

    // Отправляем subscription request
    subscribe_tx.send(request).await?;
    info!("✅ Subscription request sent successfully");
    info!("✅ Successfully subscribed, listening for Create transactions...");

    loop {
        match updates_stream.next().await {
            Some(Ok(message)) => {
                if let Some(update) = message.update_oneof {
                    if let UpdateOneof::Transaction(tx) = update {
                        // Парсим Create транзакцию
                        if let Some(parsed) = parse_create_transaction(&tx) {
                            info!("🔥 Create detected: mint={} creator={} signature={}",
                                parsed.mint_address, parsed.creator_address, parsed.signature);

                            // Отправляем через broadcast channel
                            if state.tx_sender.send(parsed).is_err() {
                                warn!("No SSE subscribers, dropping Create transaction");
                            }
                        }
                    }
                }
            }
            Some(Err(e)) => {
                error!("GRPC stream error: {} (code: {:?}, message: {})",
                    e,
                    e.code(),
                    e.message());
                if let Some(source) = Error::source(&e) {
                    error!("Source error: {}", source);
                }
                return Err(e.into());
            }
            None => {
                error!("GRPC stream closed unexpectedly (server closed connection)");
                break;
            }
        }
    }

    info!("GRPC stream ended, will reconnect with backoff...");
    Ok(())
}

fn parse_create_transaction(tx: &SubscribeUpdateTransaction) -> Option<CreateTransaction> {
    let pump_fun_program_id = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

    // Проверяем метаданные транзакции
    let tx_data = tx.transaction.as_ref()?;
    let meta = tx_data.meta.as_ref()?;

    // Проверяем логи на наличие Pump.fun и Create
    let log_messages = meta.log_messages.as_ref()?;
    let log_str = match std::str::from_utf8(log_messages) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let has_pump_fun = log_str.contains(pump_fun_program_id);
    let is_create = log_str.contains("Instruction: Create") && !log_str.contains("CreateV2");
    let is_create_v2 = log_str.contains("Instruction: CreateV2");

    if !has_pump_fun || (!is_create && !is_create_v2) {
        return None;
    }

    // Получаем подпись
    let signature = if !tx_data.signature.is_empty() {
        bs58::encode(&tx_data.signature).into_string()
    } else if let Some(tx_transaction) = tx_data.transaction.as_ref() {
        if let Some(first_sig) = tx_transaction.signatures.first() {
            bs58::encode(first_sig).into_string()
        } else {
            return None;
        }
    } else {
        return None;
    };

    // Получаем creator (первый аккаунт)
    let creator_address = if let Some(tx_transaction) = tx_data.transaction.as_ref() {
        if let Some(message) = tx_transaction.message.as_ref() {
            if let Some(first_key) = message.account_keys.first() {
                bs58::encode(first_key).into_string()
            } else {
                return None;
            }
        } else {
            return None;
        }
    } else {
        return None;
    };

    // Получаем mint из post_token_balances
    let post_balances = &meta.post_token_balances;
    let pre_balances = &meta.pre_token_balances;

    let pre_mints: std::collections::HashSet<String> = pre_balances.iter()
        .map(|b| b.mint.clone())
        .collect();

    let mut candidate_mints = vec![];
    for balance in post_balances {
        let mint = &balance.mint;
        if !pre_mints.contains(mint) && !mint.contains("11111111111111111111111111111111") {
            candidate_mints.push(mint.clone());
        }
    }

    let mint_address = candidate_mints.iter()
        .find(|m: &&String| m.ends_with("pump"))
        .or_else(|| candidate_mints.first())
        .cloned()?;

    Some(CreateTransaction {
        signature,
        mint_address,
        creator_address,
        slot: tx_data.slot,
        is_create_v2,
    })
}

