use std::sync::atomic::AtomicU64;
use qrcode::QrCode;
use image::Luma;
use crate::PHONE_TRANSFERS_DIRECTORY;

static FILE_ID: AtomicU64 = AtomicU64::new(0);

pub fn build_qr(data: &str) {
    let id = FILE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let filename = format!("{:03}.png", id);
    let path = format!("{}/{}", PHONE_TRANSFERS_DIRECTORY, filename);
    let code = QrCode::new(data.as_bytes()).unwrap();
    let image = code.render::<Luma<u8>>().min_dimensions(200, 200).build();
    image.save(&path).unwrap();
    println!("QR save as: {}", &path);
}

