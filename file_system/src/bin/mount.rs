use qrfs::{QRFileSystem, initialize_new_disk, read_u64, write_u64, INODE_COUNTER_START, BLOCK_SIZE, INODE_COUNTER, get_default_attrs, Ordering};
use std::env;
use std::path::Path;
use std::io::{self, Write};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("=== QR Filesystem Mount ===");
        println!("Usage:");
        println!("  {} <MOUNTPOINT> [disk_path]", args[0]);
        println!("\nExamples:");
        println!("  {} /mnt/qrfs", args[0]);
        println!("  {} /mnt/qrfs my_fs.bin", args[0]);
        return Ok(());
    }
    
    let mountpoint = &args[1];
    let disk_path = if args.len() > 2 { 
        args[2].clone()
    } else { 
        println!("No disk file specified.");
        print!("Enter disk file path [qrfs.bin]: ");
        io::stdout().flush().unwrap();
        
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim().to_string();
        
        if input.is_empty() {
            "qrfs.bin".to_string()
        } else {
            input
        }
    };
    
    println!("Mounting filesystem from disk: {}", disk_path);
    
    let is_new_disk = !Path::new(&disk_path).exists();
    if is_new_disk {
        println!("Creating new disk file: {}", disk_path);
        initialize_new_disk(&disk_path)?;
    } else {
        println!("Using existing disk file: {}", disk_path);
    }

    let mut fs = QRFileSystem::new(&disk_path, "null");
    fs.load_fs_from_disk()?;

    let actual_inodes: u64 = read_u64(&mut fs.disk, INODE_COUNTER_START * BLOCK_SIZE)?;
    INODE_COUNTER.store(actual_inodes + 1, Ordering::Relaxed);

    if is_new_disk {
        println!("Initializing new filesystem with root directory...");
        fs.push(1, "/".to_string(), None, 0, &get_default_attrs(1, 0, true)).unwrap();
        write_u64(&mut fs.disk, INODE_COUNTER_START * BLOCK_SIZE, 2)?;
        INODE_COUNTER.store(2, Ordering::Relaxed);
    }

    println!("Filesystem loaded with {} entries", fs.files.len());
    println!("Mounting at: {}", mountpoint);
    
    match fuser::mount2(fs, mountpoint, &[]) {
        Ok(_) => {
            println!("Mounted successfully!");
            println!("Use 'fusermount -u {}' to unmount", mountpoint);
            println!("\nYou can export this filesystem using:");
            println!("  qrfs-export {} ./export_dir your_passphrase", disk_path);
        },
        Err(e) => {
            eprintln!("Mount failed: {:?}", e);
            println!("Note: You may need to create the mountpoint directory first");
        }
    }
    
    Ok(())
}