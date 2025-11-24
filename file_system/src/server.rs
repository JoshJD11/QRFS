use axum::{routing::post, Json, Router};
use serde::Deserialize;
use std::net::SocketAddr;

use crate::builder::build_qr;

#[derive(Deserialize)]
pub struct RawQR {
    data: String
}

async fn upload_data(Json(payload): Json<RawQR>) {
    // TO DO
    build_qr(&payload.data);
}

pub async fn run_server() {
    let app = Router::new().route("/upload-data", post(upload_data));

    let addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
    println!("Rust server listening!");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
