use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    signal,
    sync::Mutex,
};
use std::{io::{self, Write}, sync::Arc};
use std::env;
use std::time::Instant;

const CMD_MSG: u8 = 0x00;
const CMD_LIST: u8 = 0x01;
const CMD_TO: u8 = 0x02;
const CMD_EXCEPT: u8 = 0x03;
const CMD_BAN: u8 = 0x04;
const CMD_PING: u8 = 0x05;
const CMD_PONG: u8 = 0x06;
const CMD_INVALID: u8 = 0xFF;


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    //Start---------------- Get nickname by command line-------------------
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: cargo run -- <nickname>");
        std::process::exit(1);
    }
    let nickname = args[1].clone();
    //End---------------- Get nickname by command line-------------------

    //Start--------------------Variables--------------------------
    let stream = TcpStream::connect("localhost:25229").await?;
    let (reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));
    let mut server_reader = BufReader::new(reader).lines();
    let stdin_reader = BufReader::new(tokio::io::stdin());
    let mut user_input = stdin_reader.lines();
    let mut start_time = Arc::new(Mutex::new(Instant::now()));
    //END--------------------Variables--------------------------


    ////Start--------------------SEND NICKNAME--------------------------
    {
        let mut w = writer.lock().await;
        w.write_all(format!("{}\n", nickname).as_bytes()).await?;
    }
    //END--------------------Variables--------------------------

    println!("--- Chat started. Type messages ---");

    //Start--------------------Ctrl + C--------------------------
    tokio::spawn(async move {
        signal::ctrl_c().await.unwrap();
        println!("\nGoodbye!");
        std::process::exit(0);
    });
    //END--------------------Ctrl + C--------------------------


    //Start--------------------MainLoop--------------------------
    loop {
        tokio::select! {
            //read from server
            Ok(Some(msg)) = server_reader.next_line() => {
                let msg_bytes = msg.as_bytes();
                let cmd = msg_bytes[0];
                if msg == "::server::shutdown"{
                    println!("disconnected!");
                    break;
                }
                else if cmd==CMD_PONG{
                    let time = start_time.lock().await;
                    let elapsed = time.elapsed();
                    println!("ping RTT: {:.3} ms\n", elapsed.as_secs_f64() * 1000.0);
                }
                else if !msg.starts_with(&format!("[{}]", nickname)) {
                    println!("{}", msg);
                }
            }
            Ok(Some(mut line)) = user_input.next_line() => {

                let cmd = parse_command(&line);
                if cmd == CMD_INVALID{
                    println!("Invalid command!");
                    continue;
                }
                else if cmd != CMD_MSG{
                    if let Some((_first_word, rest)) = line.split_once(' ') {
                        line = rest.to_string(); // 재할당
                    }

                }
                if cmd == CMD_PING{
                    let mut time = start_time.lock().await;
                    *time = Instant::now();
                }
                let payload = format!("{}\n", line);
                let mut msg = Vec::with_capacity(1+payload.len());

                msg.push(cmd);
                msg.extend_from_slice(payload.as_bytes());

                let mut w = writer.lock().await;
                w.write_all(&msg).await?;
            }
            else => {
                println!("async select dead!");
                break;
            }
        }
    }
    //END--------------------MainLoop--------------------------

    Ok(())
}

fn parse_command(input: &str) -> u8{
    if !input.starts_with('\\'){
        return CMD_MSG;
    }

    let parts: Vec<&str> = input[1..].splitn(2,' ').collect();
    let cmd = parts[0];
    
    match cmd{
        "list" => CMD_LIST,
        "to" => CMD_TO,
        "except" => CMD_EXCEPT,
        "ban" => CMD_BAN,
        "ping" => CMD_PING,
        _ => CMD_INVALID,
    }
}