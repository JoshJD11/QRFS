use std::sync::atomic::AtomicU64;
use qrcode::QrCode;
use image::Luma;

static FILE_ID: AtomicU64 = AtomicU64::new(1);

pub fn build_qr(data: &str) {
    let id = FILE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let filename = format!("{}.png", id);
    let code = QrCode::new(data.as_bytes()).unwrap();
    let image = code.render::<Luma<u8>>().min_dimensions(200, 200).build();
    image.save(&filename).unwrap();
    println!("QR save as: {}", &filename);
}

