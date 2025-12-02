use qrfs::{QRFileSystem, initialize_new_disk, write_u64, INODE_COUNTER_START, BLOCK_SIZE, INODE_COUNTER, Ordering};
use std::env;
use std::path::Path;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("=== QR Filesystem Import ===");
        println!("Usage:");
        println!("  {} <input_dir> [passphrase] [disk_path]", args[0]);
        println!("\nExamples:");
        println!("  {} ./qr_codes", args[0]);
        println!("  {} ./qr_codes mypassword imported_fs.bin", args[0]);
        return Ok(());
    }
    
    let input_dir = &args[1];
    let passphrase = if args.len() > 2 { &args[2] } else { "test123" };
    let disk_path = if args.len() > 3 { &args[3] } else { "imported_fs.bin" };
    
    println!("Importing filesystem from QR codes in: {}", input_dir);
    
    let is_new_disk = !Path::new(disk_path).exists();
    if is_new_disk {
        println!("Creating new disk file: {}", disk_path);
        initialize_new_disk(disk_path)?;
    }

    let mut fs = QRFileSystem::new(disk_path, "null");
    if let Err(e) = fs.import_files_from_qr(input_dir, passphrase) {
        eprintln!("Import failed: {}", e);
        return Ok(());
    }

    write_u64(&mut fs.disk, INODE_COUNTER_START * BLOCK_SIZE, INODE_COUNTER.load(Ordering::Relaxed))?;
    
    println!("\n=== Import completed successfully! ===");
    println!("Filesystem imported to: {}", disk_path);
    println!("Total entries: {}", fs.files.len());
    println!("Inode counter: {}", INODE_COUNTER.load(Ordering::Relaxed));
    println!("\nYou can now mount this filesystem using:");
    println!("  qrfs-mount /mnt/qrfs {}", disk_path);
    
    Ok(())
}