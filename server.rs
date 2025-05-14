
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

type Tx = broadcast::Sender<String>;

use tokio::net::tcp::OwnedWriteHalf;

struct ClientInfo {
    addr: std::net::SocketAddr,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    cancel_token: CancellationToken,
}
type Clients = Arc<Mutex<HashMap<String, ClientInfo>>>;

const CMD_MSG: u8 = 0x00;
const CMD_LIST: u8 = 0x01;
const CMD_TO: u8 = 0x02;
const CMD_EXCEPT: u8 = 0x03;
const CMD_BAN: u8 = 0x04;
const CMD_PING: u8 = 0x05;
const CMD_PONG: u8 = 0x06;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let clients: Clients = Arc::new(Mutex::new(HashMap::new()));
    let server_ip_port = "localhost:25229";
    let listener = TcpListener::bind(server_ip_port).await?;
    let (tx, _rx) = broadcast::channel::<String>(100);

    println!("Server running on {}", server_ip_port);

    let cancel_token = CancellationToken::new();
    let shutdown_token = cancel_token.clone();
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.unwrap();
        println!("Ctrl+C received!");
        shutdown_token.cancel();
    };

    let server_loop = async {
        loop {
            let (stream, _addr) = listener.accept().await?;
            let tx = tx.clone();
            let rx = tx.subscribe();
            let clients = clients.clone();
            let token = cancel_token.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_client(stream, tx, rx, clients, token).await {
                    eprintln!("Error handling client: {:?}", e);
                }
            });
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        _ = ctrl_c => {
            println!("[*] Cleaning up resources...");
        }
        res = server_loop => {
            if let Err(e) = res {
                eprintln!("Server error: {:?}", e);
            }
        }
    }

    Ok(())
}

async fn handle_client(
    stream: TcpStream,
    tx: Tx,
    mut rx: broadcast::Receiver<String>,
    clients: Clients,
    cancel_token: CancellationToken,
) -> anyhow::Result<()> {
    let server_addr = stream.local_addr()?;
    let client_addr = stream.peer_addr()?;
    let (reader_raw, writer_raw) = stream.into_split();
    let mut reader = BufReader::new(reader_raw).lines();
    let writer = Arc::new(Mutex::new(writer_raw));
    let local_cancel_token = CancellationToken::new();

    let nickname = match reader.next_line().await {
        Ok(Some(name)) => name,
        _ => {
            let mut w = writer.lock().await;
            w.write_all(b"Invalid nickname.").await.ok();
            return Ok(());
        }
    };

    {
        let (is_full, is_dup) = {
            let map = clients.lock().await;
            (map.len() >= 4, map.contains_key(&nickname))
        };

        if is_full {
            let mut w = writer.lock().await;
            w.write_all(b"Server full. Try again later.").await.ok();
            return Ok(());
        }
        if is_dup {
            let mut w = writer.lock().await;
            w.write_all(b"Nickname already in use.").await.ok();
            return Ok(());
        }

        let mut map = clients.lock().await;
        map.insert(nickname.clone(), ClientInfo {
            addr: client_addr,
            writer: writer.clone(),
            cancel_token: local_cancel_token.clone(),
        });
    }

    {
        let map = clients.lock().await;
        let len = map.len();
        let welcome_msg = format!("Welcome <{}> to the chat room at <{}>. There are <{}> users.\n", nickname, server_addr, len);
        let mut w = writer.lock().await;
        w.write_all(welcome_msg.as_bytes()).await?;
        println!("<{}> joined from <{}>. {} users online.\n", nickname, client_addr, len);
    }

    tokio::spawn({
        let writer = writer.clone();
        let cancel_token = cancel_token.clone();
        let nickname = nickname.clone();
        let local_cancel_token = local_cancel_token.clone();
        async move {
            loop {
                tokio::select! {
                    _ = local_cancel_token.cancelled() => break,
                    _ = cancel_token.cancelled() => break,
                    Ok(msg) = rx.recv() => {
                        let mut w = writer.lock().await;
                        if w.write_all(msg.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                }
            }
            println!("[{}'s] writer thread dead.", nickname);
        }
    });

    loop {
        tokio::select! {
            _ = local_cancel_token.cancelled() => break,
            _ = cancel_token.cancelled() => break,
            result = reader.next_line() => {
                match result {
                    Ok(Some(line)) => {
                        let msg_bytes = line.as_bytes();
                        let cmd = msg_bytes[0];
                        let payload = &msg_bytes[1..];
                        let payload_str = match std::str::from_utf8(payload) {
                            Ok(s) => s.to_string(),
                            Err(_) => {
                                eprintln!("Invalid UTF-8 in payload");
                                continue;
                            }
                        };

                        match cmd {
                            CMD_LIST => {
                                let map = clients.lock().await;
                                let mut list_msg = String::from("== Client List ==\n");
                                for (nick, info) in map.iter() {
                                    list_msg += &format!("{} : {}\n", nick, info.addr);
                                }
                                let mut w = writer.lock().await;
                                w.write_all(list_msg.as_bytes()).await.ok();
                            }
                            CMD_TO => {
                                if let Some((target, msg)) = payload_str.split_once(' ') {
                                    let map = clients.lock().await;
                                    if !map.contains_key(target){
                                        let mut w = writer.lock().await;
                                        let dm = format!("No name {} in the server!\n", target);
                                        w.write_all(dm.as_bytes()).await.ok();
                                        continue;
                                    }
                                    if let Some(target_info) = map.get(target) {
                                        let mut tw = target_info.writer.lock().await;
                                        let dm = format!("[whisper from {}]: {}\n", nickname, msg);
                                        tw.write_all(dm.as_bytes()).await.ok();
                                    }
                                }
                            }
                            CMD_EXCEPT => {
                                if let Some((exclude, msg)) = payload_str.split_once(' ') {
                                    let map = clients.lock().await;
                                    if !map.contains_key(exclude){
                                        let mut w = writer.lock().await;
                                        let dm = format!("No name {} in the server!\n", exclude);
                                        w.write_all(dm.as_bytes()).await.ok();
                                        continue;
                                    }
                                    for (nick, info) in map.iter() {
                                        if nick != exclude{
                                            let mut w = info.writer.lock().await;
                                            let out_msg = format!("[except {}]: {}\n", nick, msg);
                                            w.write_all(out_msg.as_bytes()).await.ok();
                                        }
                                    }
                                }
                            }
                            CMD_BAN => {
                                let ban_target = payload_str.trim();
                                let mut map = clients.lock().await;
                                if let Some(info) = map.remove(ban_target) {
                                    info.cancel_token.cancel();
                                    let mut banned_writer = info.writer.lock().await;
                                    let msg = format!("you are banned by {}\n", nickname);
                                    banned_writer.write_all(msg.as_bytes()).await.ok();
                                }
                            }
                            CMD_PING => {
                                let mut w = writer.lock().await;
                                w.write_all(&[CMD_PONG,b'\n']).await.ok();
                            }
                            CMD_MSG => {
                                let full_msg = format!("[{}]: {}\n", nickname, payload_str);
                                if payload_str.contains("i hate professor"){
                                    let mut w = writer.lock().await;
                                    let ban_msg = format!("you are banned by server : unappropriate word\n");
                                    w.write_all(ban_msg.as_bytes()).await.ok();
                                    local_cancel_token.cancel();
                                    break;
                                }
                                tx.send(full_msg).ok();
                            }
                            _ => {
                                let mut w = writer.lock().await;
                                let err = format!("invalid command: {}\n", payload_str);
                                w.write_all(err.as_bytes()).await.ok();
                            }
                        }
                    }
                    _ => break,
                }
            }
        }
    }

    {
        let mut map = clients.lock().await;
        map.remove(&nickname);
        let len = map.len();
        let msg = format!("<{}> has left the room. {} users remain.\n", nickname, len);
        println!("{}", msg);
        tx.send(msg).ok();
    }

    {
        let mut w = writer.lock().await;
        w.write_all(b"::server::shutdown").await.ok();
    }

    Ok(())
}
