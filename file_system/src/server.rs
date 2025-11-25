use axum::{routing::post, Json, Router};
use serde::Deserialize;
use std::net::SocketAddr;

use crate::builder::build_qr;

#[derive(Deserialize)]
pub struct RawQR {
    data: String
}

async fn upload_data(Json(payload): Json<RawQR>) {
    build_qr(&payload.data);
}

async fn finish_scanning() {
    // Klob must cook here
    println!("All QR codes were scanned"); // debug
}

pub async fn run_server() {
    let app = Router::new()
        .route("/upload-data", post(upload_data))
        .route("/finish-scanning", post(finish_scanning));

    let addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
    println!("Rust server listening!");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
