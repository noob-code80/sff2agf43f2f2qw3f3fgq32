# Как запустить Rust GRPC Proxy

## Важно!
Это **НЕ** `solana_geyser_test` из `C:\test`!

Это **`rust-grpc-proxy.exe`** который слушает TCP socket на порту **8725**.

## Сборка

```bash
cd rust_grpc_proxy
cargo build --release --target x86_64-pc-windows-gnu
```

Бинарник будет в: `target/x86_64-pc-windows-gnu/release/rust-grpc-proxy.exe`

## Запуск

```bash
./target/x86_64-pc-windows-gnu/release/rust-grpc-proxy.exe
```

Или если уже собран:

```bash
rust-grpc-proxy.exe
```

## Что должно быть в логах:

```
🚀 Rust GRPC Proxy starting (MAX SPEED MODE - TCP socket)...
🚀 Rust GRPC Proxy TCP server started on port 8725
⚡ MAX SPEED: Direct TCP socket (no HTTP overhead)
📡 TCP endpoint: localhost:8725
Connecting to GRPC endpoint: https://fr.grpc.gadflynode.com:25565
✅ GRPC channel connected successfully
```

## НЕ запускайте:
- ❌ `solana_geyser_test.exe` (это тестовая программа, порт 8724, HTTP/SSE)
- ❌ `node grpc_proxy.js` (старый JS прокси)

## Запускайте:
- ✅ `rust-grpc-proxy.exe` (новый Rust прокси, порт 8725, TCP socket)

