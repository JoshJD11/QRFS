use qrfs::{QRFileSystem, initialize_new_disk, read_u64, write_u64, 
           INODE_COUNTER_START, BLOCK_SIZE, INODE_COUNTER, 
           get_default_attrs, Ordering};
use std::env;
use std::path::Path;
use std::io::{self, Write};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 3 {
        println!("=== QR Filesystem Mount ===");
        println!("Usage:");
        println!("  {} <qr_directory> <mount_point>", args[0]);
        println!("\nExamples:");
        println!("  {} ./qr_codes /mnt/qrfs", args[0]);
        println!("  {} ~/backups/qr /home/user/mount", args[0]);
        return Ok(());
    }
    
    let qr_directory = &args[1];
    let mountpoint = &args[2];
    
    print!("Enter passphrase: ");
    io::stdout().flush().unwrap();
    let mut passphrase = String::new();
    io::stdin().read_line(&mut passphrase).unwrap();
    let passphrase = passphrase.trim().to_string();
    
    if passphrase.is_empty() {
        eprintln!("Error: Passphrase cannot be empty");
        return Ok(());
    }
    
    println!("=== QR Filesystem Mount ===");
    println!("QR directory: {}", qr_directory);
    println!("Mount point: {}", mountpoint);
    println!("Passphrase: {}", if passphrase.len() > 5 { 
        format!("{}...", &passphrase[..3]) 
    } else { 
        "***".to_string() 
    });
    
    let temp_dir = std::env::temp_dir();
    let pid = std::process::id();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let disk_path = temp_dir.join(format!("qrfs_temp_{}_{}.bin", pid, timestamp));
    let disk_path_str = disk_path.to_str().unwrap();
    
    println!("Temporary disk: {}", disk_path_str);
    
    let qr_dir_path = Path::new(qr_directory);
    if qr_dir_path.exists() {
        println!("Found QR directory, attempting import...");
        
        initialize_new_disk(disk_path_str)?;
        let mut fs = QRFileSystem::new(disk_path_str, qr_directory);
        
        match fs.import_files_from_qr(qr_directory, &passphrase) {
            Ok(_) => {
                // Update inode counter on disk
                write_u64(&mut fs.disk, INODE_COUNTER_START * BLOCK_SIZE, INODE_COUNTER.load(Ordering::Relaxed))?;
                println!("Successfully imported from QR codes!");
                println!("Filesystem has {} entries", fs.files.len());
                
                // Enable auto-export on unmount
                fs.enable_auto_export(qr_directory, &passphrase);
                
                // Mount the imported filesystem
                println!("Mounting at: {}", mountpoint);
                match fuser::mount2(fs, mountpoint, &[]) {
                    Ok(_) => {
                        println!("\nMounted successfully!");
                        println!("Filesystem imported from: {}", qr_directory);
                        println!("Use 'fusermount -u {}' to unmount", mountpoint);
                        println!("\nNote: Changes will be auto-exported to QR codes on unmount");
                    },
                    Err(e) => {
                        eprintln!("Mount failed: {:?}", e);
                        println!("Note: You may need to create the mountpoint directory first");
                    }
                }
            }
            Err(e) => {
                println!("Import failed: {}", e);
                println!("Creating new filesystem instead...");
                create_new_filesystem(disk_path_str, mountpoint, qr_directory, &passphrase)?;
            }
        }
    } else {
        println!("QR directory does not exists...");
    }
    
    Ok(())
}

fn create_new_filesystem(disk_path: &str, mountpoint: &str, qr_directory: &str, passphrase: &str) -> std::io::Result<()> {
    initialize_new_disk(disk_path)?;
    
    let mut fs = QRFileSystem::new(disk_path, qr_directory);
    
    fs.push(1, "/".to_string(), None, 0, &get_default_attrs(1, 0, true)).unwrap();
    write_u64(&mut fs.disk, INODE_COUNTER_START * BLOCK_SIZE, 2)?;
    INODE_COUNTER.store(2, Ordering::Relaxed);
    
    fs.enable_auto_export(qr_directory, passphrase);
    
    println!("Initialized new filesystem with root directory");
    println!("Mounting at: {}", mountpoint);
    
    match fuser::mount2(fs, mountpoint, &[]) {
        Ok(_) => {
            println!("\nMounted successfully!");
            println!("New filesystem created");
            println!("QR codes will be saved to: {}", qr_directory);
            println!("Use 'fusermount -u {}' to unmount", mountpoint);
            println!("\nNote: Changes will be auto-exported to QR codes on unmount");
        },
        Err(e) => {
            eprintln!("Mount failed: {:?}", e);
            println!("Note: You may need to create the mountpoint directory first");
        }
    }
    
    Ok(())
}