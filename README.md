# Chatting
## How to compile?
```
cargo new <name>

//server
cargo run

//client
cargo run -- <nickname>
```
## Client Command
- just message : send to all clients
- \to <nick> <text> : whisper
- \except <nick> <text> : send <text> to everyone except <nick>
- \ban <nick> : ban <nick> form server
- \ping : RTT time
- \list : clients in server

## Cargo.toml depedencies
```
[dependencies]
tokio = { version = "1", features = ["full"] }
tokio-util = "0.7"
anyhow = "1.0"
ctrlc = "3.4"
```

# video link
https://youtu.be/2Fvmp7OmE94
