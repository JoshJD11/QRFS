use qrfs::{QRFileSystem, initialize_new_disk, read_u64, INODE_COUNTER_START, BLOCK_SIZE, INODE_COUNTER, Ordering};
use std::env;
use std::path::Path;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("=== QR Filesystem Export ===");
        println!("Usage:");
        println!("  {} <disk_path> [output_dir] [passphrase]", args[0]);
        println!("\nExamples:");
        println!("  {} my_fs.bin", args[0]);
        println!("  {} my_fs.bin ./qr_codes mypassword", args[0]);
        return Ok(());
    }
    
    let disk_path = &args[1];
    let output_dir = if args.len() > 2 { &args[2] } else { "./qr_export" };
    let passphrase = if args.len() > 3 { &args[3] } else { "test123" };
    
    println!("Loading filesystem from disk: {}", disk_path);
    
    let is_new_disk = !Path::new(disk_path).exists();
    if is_new_disk {
        println!("Creating new disk file: {}", disk_path);
        initialize_new_disk(disk_path)?;
    }

    let mut fs = QRFileSystem::new(disk_path, "null");
    fs.load_fs_from_disk()?;

    let actual_inodes: u64 = read_u64(&mut fs.disk, INODE_COUNTER_START * BLOCK_SIZE)?;
    INODE_COUNTER.store(actual_inodes + 1, Ordering::Relaxed);

    println!("Filesystem loaded with {} entries", fs.files.len());
    
    println!("\nExporting to: {}", output_dir);
    if let Err(e) = fs.export_files_as_qr(output_dir, passphrase) {
        eprintln!("Export failed: {}", e);
        return Ok(());
    }
    
    println!("\n=== Export completed successfully! ===");
    println!("QR codes saved to: {}", output_dir);
    println!("Passphrase: '{}'", passphrase);
    println!("You can import this filesystem using:");
    println!("  qrfs-import {} {} new_fs.bin", output_dir, passphrase);
    
    Ok(())
}