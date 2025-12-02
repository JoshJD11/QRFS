use qrfs::{QRFileSystem, initialize_new_disk, read_u64, write_u64, 
           INODE_COUNTER_START, BLOCK_SIZE, INODE_COUNTER, 
           get_default_attrs, Ordering};
use std::env;
use std::path::Path;
use std::io::{self, Write};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("=== QR Filesystem Mount ===");
        println!("Usage:");
        println!("  {} <qr_directory> ", args[0]);
        println!("\nExamples:");
        println!("  {} ./qr_codes", args[0]);
        println!("  {} ~/backups/qr", args[0]);
        return Ok(());
    }
    
    let qr_directory = &args[1];
    
    print!("Enter passphrase: ");
    io::stdout().flush().unwrap();
    let mut passphrase = String::new();
    io::stdin().read_line(&mut passphrase).unwrap();
    let passphrase = passphrase.trim().to_string();
    
    if passphrase.is_empty() {
        eprintln!("Error: Passphrase cannot be empty");
        return Ok(());
    }
    
    
    let temp_dir = std::env::temp_dir();
    let pid = std::process::id();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let disk_path = temp_dir.join(format!("qrfs_temp_{}_{}.bin", pid, timestamp));
    let disk_path_str = disk_path.to_str().unwrap();
    
    // println!("Temporary disk: {}", disk_path_str);
    
    let qr_dir_path = Path::new(qr_directory);
    if qr_dir_path.exists() {
        println!("QR directory already exists...");
    } else {
        println!("Creating new QRFS in {}...", qr_directory);
        create_new_filesystem(disk_path_str, qr_directory, &passphrase)?;
    }
    
    Ok(())
}

fn create_new_filesystem(disk_path: &str, qr_directory: &str, passphrase: &str) -> std::io::Result<()> {
    initialize_new_disk(disk_path)?;
    
    let mut fs = QRFileSystem::new(disk_path, qr_directory);
    
    fs.push(1, "/".to_string(), None, 0, &get_default_attrs(1, 0, true)).unwrap();
    write_u64(&mut fs.disk, INODE_COUNTER_START * BLOCK_SIZE, 2)?;
    INODE_COUNTER.store(2, Ordering::Relaxed);
    
    fs.enable_auto_export(qr_directory, passphrase);
    fs.export_files_as_qr(qr_directory, passphrase);
    
    
    Ok(())
}