// src/bin/lector.rs
use qrfs::{QRFileSystem, initialize_new_disk};
use std::env;
use std::path::Path;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::sync::OnceLock;
use axum::{routing::post, Json, Router};
use serde::Deserialize;
use std::net::SocketAddr;
use qrcode::QrCode;
use image::Luma;

#[derive(Debug)]
struct ServerState {
    scanning_complete: bool,
    total_qrs: u32,
}

impl ServerState {
    fn new() -> Self {
        Self {
            scanning_complete: false,
            total_qrs: 0,
        }
    }
}

static SERVER_STATE: OnceLock<Arc<Mutex<ServerState>>> = OnceLock::new();
static QR_DIR: OnceLock<String> = OnceLock::new();

#[derive(Deserialize)]
struct QRData {
    data: String,
}

async fn receive_qr(Json(payload): Json<QRData>) -> &'static str {
    let state = SERVER_STATE.get().unwrap().clone();
    let mut state = state.lock().unwrap();
    
    let id = state.total_qrs;
    state.total_qrs += 1;
    
    let filename = format!("{:03}.png", id);
    let qr_dir = QR_DIR.get().unwrap();
    let path = format!("{}/{}", qr_dir, filename);
    
    if let Ok(code) = QrCode::new(payload.data.as_bytes()) {
        let image = code.render::<Luma<u8>>()
            .min_dimensions(200, 200)
            .build();
        
        let _ = std::fs::create_dir_all(qr_dir);
        let _ = image.save(&path);
    }
    
    "OK"
}

async fn scanning_done() -> &'static str {
    let state = SERVER_STATE.get().unwrap().clone();
    let mut state = state.lock().unwrap();
    state.scanning_complete = true;
    "DONE"
}

async fn run_server() {
    let app = Router::new()
        .route("/qr", post(receive_qr))
        .route("/done", post(scanning_done));

    let addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
    println!("Server ready at port 3000");
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("Usage: {} <qr_directory>", args[0]);
        return Ok(());
    }
    
    let qr_dir = args[1].clone();
    QR_DIR.set(qr_dir.clone()).unwrap();
    
    print!("Passphrase: ");
    io::stdout().flush().unwrap();
    let mut passphrase = String::new();
    io::stdin().read_line(&mut passphrase).unwrap();
    let passphrase = passphrase.trim().to_string();
    
    if passphrase.is_empty() {
        eprintln!("Passphrase required");
        return Ok(());
    }
    
    let state = Arc::new(Mutex::new(ServerState::new()));
    SERVER_STATE.set(state.clone()).unwrap();
    
    let _ = std::fs::create_dir_all(&qr_dir);
    
    let server_thread = std::thread::spawn(|| {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async {
                run_server().await;
            });
    });
    
    println!("Waiting for QR codes... (send to /done when finished)");
    
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        
        let state_ref = state.lock().unwrap();
        if state_ref.scanning_complete {
            println!("Received {} QR codes", state_ref.total_qrs);
            break;
        }
    }
    
    drop(server_thread);
    
    let has_qr_files = Path::new(&qr_dir)
        .read_dir()
        .map_or(false, |entries| {
            entries.filter_map(|e| e.ok())
                .any(|e| e.path().extension().map(|ext| ext == "png").unwrap_or(false))
        });
    
    if !has_qr_files {
        eprintln!("No QR codes received");
        return Ok(());
    }
    
    print!("Mount point: ");
    io::stdout().flush().unwrap();
    let mut mountpoint = String::new();
    io::stdin().read_line(&mut mountpoint).unwrap();
    let mountpoint = mountpoint.trim();
    
    if mountpoint.is_empty() {
        eprintln!("No Mount point provided, exiting");
        return Ok(());
    }
    
    let temp_disk = format!("/tmp/qrfs_lector_{}.bin", std::process::id());
    initialize_new_disk(&temp_disk)?;
    
    let mut fs = QRFileSystem::new(&temp_disk, mountpoint);
    
    match fs.import_files_from_qr(&qr_dir, &passphrase) {
        Ok(_) => {
            match fuser::mount2(fs, mountpoint, &[]) {
                Ok(_) => {
                    println!("Mounted at {}", mountpoint);
                }
                Err(e) => eprintln!("Mount failed: {}", e),
            }
        }
        Err(e) => eprintln!("Import failed: {}", e),
    }
    
    let _ = std::fs::remove_file(&temp_disk);
    
    Ok(())
}