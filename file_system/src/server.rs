use axum::{routing::post, Json, Router};
use serde::Deserialize;
use std::net::SocketAddr;

// use crate::builder::construir_imagen;

#[derive(Deserialize)]
pub struct ImagePayload {
    pub mime: String,
    pub data: String, // base64
}

async fn upload_image(Json(payload): Json<ImagePayload>) -> &'static str {
    // TO DO
    println!("{}", &payload.mime); // these prints are only for debug
    println!("{}", &payload.data); // reconstruction must be implemented
    "ok"
}

pub async fn run_server() {
    let app = Router::new().route("/upload-image", post(upload_image));

    let addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
    println!("Servidor Rust escuchando en http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
