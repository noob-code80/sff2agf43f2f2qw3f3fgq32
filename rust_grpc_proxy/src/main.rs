// Rust GRPC Proxy - заменяет grpc_proxy.js
// Подключается к GRPC напрямую и отправляет Create транзакции через TCP socket (максимальная скорость!)

mod grpc_client;

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::net::TcpListener;
use tokio::io::AsyncWriteExt;
use tracing::{info, error};
use futures::StreamExt;
use anyhow::Context;
use bincode;

// Структура Create транзакции (совместима с grpc_proxy.js)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTransaction {
    pub signature: String,
    pub mint_address: String,
    pub creator_address: String,
    pub slot: u64,
    pub is_create_v2: bool,
}

// Состояние приложения
#[derive(Clone)]
pub struct AppState {
    pub tx_sender: broadcast::Sender<CreateTransaction>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Устанавливаем CryptoProvider для rustls
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");

    // Инициализируем логирование
    let filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("rust_grpc_proxy=info".parse().unwrap())
        .add_directive("grpc_client=info".parse().unwrap());

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_ansi(true)
        .init();

    info!("🚀 Rust GRPC Proxy starting (MAX SPEED MODE - TCP socket)...");

    // Создаем broadcast channel для TCP клиентов
    let (tx, _) = broadcast::channel::<CreateTransaction>(10000);
    let state = Arc::new(AppState {
        tx_sender: tx,
    });

    // Запускаем GRPC подписку
    let grpc_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = grpc_client::run_grpc_subscription(grpc_state).await {
            error!("GRPC subscription failed: {}", e);
        }
    });

    // Запускаем TCP сервер для максимальной скорости
    let tcp_listener = TcpListener::bind("0.0.0.0:8725")
        .await
        .context("Failed to bind TCP socket to port 8725")?;

    info!("🚀 Rust GRPC Proxy TCP server started on port 8725");
    info!("⚡ MAX SPEED: Direct TCP socket (no HTTP overhead)");
    info!("📡 TCP endpoint: localhost:8725");

    // Принимаем TCP подключения
    loop {
        match tcp_listener.accept().await {
            Ok((stream, addr)) => {
                info!("✅ New TCP connection from: {}", addr);
                let state_clone = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_tcp_client(stream, state_clone).await {
                        error!("TCP client error: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept TCP connection: {}", e);
            }
        }
    }
}

// Обработка TCP клиента - максимальная скорость через бинарный протокол
async fn handle_tcp_client(
    mut stream: tokio::net::TcpStream,
    state: Arc<AppState>,
) -> anyhow::Result<()> {
    use tokio_stream::wrappers::BroadcastStream;
    
    let rx = state.tx_sender.subscribe();
    let mut broadcast_stream = BroadcastStream::new(rx);
    
    // Отправляем Create транзакции через TCP с бинарной сериализацией (bincode)
    while let Some(result) = broadcast_stream.next().await {
        match result {
            Ok(create_tx) => {
                // Сериализуем в бинарный формат (быстрее чем JSON)
                match bincode::serialize(&create_tx) {
                    Ok(data) => {
                        // Отправляем длину данных (4 байта) + данные
                        let len = data.len() as u32;
                        let len_bytes = len.to_le_bytes();
                        
                        // Записываем длину и данные
                        if let Err(e) = stream.write_all(&len_bytes).await {
                            error!("Failed to write length: {}", e);
                            break;
                        }
                        if let Err(e) = stream.write_all(&data).await {
                            error!("Failed to write data: {}", e);
                            break;
                        }
                        if let Err(e) = stream.flush().await {
                            error!("Failed to flush: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize CreateTransaction: {}", e);
                    }
                }
            }
            Err(_) => {
                // Broadcast channel закрыт
                break;
            }
        }
    }
    
    info!("TCP client disconnected");
    Ok(())
}

