use qrfs::{QRFileSystem, initialize_new_disk, write_u64, INODE_COUNTER_START, BLOCK_SIZE, INODE_COUNTER, fixed_name_to_str, Ordering};
use std::env;
use std::path::Path;
use std::io::{self, Write};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("=== QR Filesystem Consistency Check ===");
        println!("Usage:");
        println!("  {} <qr_directory>", args[0]);
        println!("\nExamples:");
        println!("  {} ./qr_codes", args[0]);
        println!("  {} ~/backups/qr", args[0]);
        return Ok(());
    }
    
    let qr_directory = &args[1];
    
    let qr_dir_path = Path::new(qr_directory);
    if !qr_dir_path.exists() {
        eprintln!("Error: QR directory '{}' does not exist", qr_directory);
        return Ok(());
    }
    
    print!("Enter passphrase: ");
    io::stdout().flush().unwrap();
    let mut passphrase = String::new();
    io::stdin().read_line(&mut passphrase).unwrap();
    let passphrase = passphrase.trim().to_string();
    
    if passphrase.is_empty() {
        eprintln!("Error: Passphrase cannot be empty");
        return Ok(());
    }
    
    println!("=== QR Filesystem Consistency Check ===");
    println!("Checking: {}", qr_directory);
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
    let disk_path = temp_dir.join(format!("qrfs_fsck_{}_{}.bin", pid, timestamp));
    let disk_path_str = disk_path.to_str().unwrap();
    
    println!("Temporary disk: {}", disk_path_str);
    
    let mut has_qr_files = false;
    if let Ok(entries) = std::fs::read_dir(qr_directory) {
        for entry in entries.flatten() {
            if let Some(ext) = entry.path().extension() {
                if ext == "png" {
                    has_qr_files = true;
                    break;
                }
            }
        }
    }
    
    if !has_qr_files {
        println!("️  No QR files (.png) found in directory");
        println!("   Directory exists but contains no QR code images");
        return Ok(());
    }
    
    println!("\n[1/4] Attempting to import filesystem from QR codes...");
    
    initialize_new_disk(disk_path_str)?;
    let mut fs = QRFileSystem::new(disk_path_str, "null");
    
    match fs.import_files_from_qr(qr_directory, &passphrase) {
        Ok(_) => {
            write_u64(&mut fs.disk, INODE_COUNTER_START * BLOCK_SIZE, INODE_COUNTER.load(Ordering::Relaxed))?;
            
            println!("✓ Successfully imported from QR codes!");
            println!("  Total entries: {}", fs.files.len());
            
            run_consistency_checks(&fs, qr_directory);
        }
        Err(e) => {
            eprintln!("✗ Failed to import filesystem: {}", e);
            println!("\n=== QR files are corrupted or invalid ===");
            
            println!("\nTroubleshooting tips:");
            println!("1. Check if passphrase is correct");
            println!("2. Ensure all QR code files are intact");
            println!("3. Verify directory contains complete set of QR codes");
        }
    }
    
    if std::path::Path::new(disk_path_str).exists() {
        let _ = std::fs::remove_file(disk_path_str);
    }
    
    Ok(())
}

fn run_consistency_checks(fs: &QRFileSystem, qr_directory: &str) {
    println!("\n[2/4] Checking filesystem structure...");
    
    let mut issues = Vec::new();
    let mut warnings = Vec::new();
    
    if !fs.files.contains_key(&1) {
        issues.push("Missing root directory (inode 1)".to_string());
    }
    
    let mut orphan_count = 0;
    for (inode, file) in &fs.files {
        if file.parent != 0 && !fs.files.contains_key(&file.parent) {
            issues.push(format!(
                "Orphaned file: inode {} ('{}') has non-existent parent {}", 
                inode, fixed_name_to_str(&file.name), file.parent
            ));
            orphan_count += 1;
        }
    }
    
    let mut visited = std::collections::HashSet::new();
    let mut has_cycle = false;
    
    for (inode, _) in &fs.files {
        if !visited.contains(inode) {
            let mut current = *inode;
            let mut path = vec![current];
            
            while current != 0 {
                if let Some(parent_file) = fs.files.get(&current) {
                    current = parent_file.parent;
                    if path.contains(&current) {
                        issues.push("Cycle detected in filesystem hierarchy".to_string());
                        has_cycle = true;
                        break;
                    }
                    path.push(current);
                } else {
                    break;
                }
            }
            visited.extend(path);
        }
    }
    
    let inode_count = fs.files.len();
    let unique_inodes: std::collections::HashSet<_> = fs.files.keys().collect();
    if inode_count != unique_inodes.len() {
        issues.push(format!("Duplicate inodes found: {} entries but {} unique inodes", 
                           inode_count, unique_inodes.len()));
    }
    
    for (inode, file) in &fs.files {
        let name = fixed_name_to_str(&file.name);
        if name.is_empty() {
            warnings.push(format!("Empty filename for inode {}", inode));
        } else if name.contains('/') && name.len() > 1 {
            issues.push(format!("Invalid filename '{}' contains '/' (inode {})", name, inode));
        } else if name == "." || name == ".." {
            issues.push(format!("Reserved filename '{}' used (inode {})", name, inode));
        }
    }
    
    for (inode, file) in &fs.files {
        for child_inode in &file.children {
            if let Some(child) = fs.files.get(child_inode) {
                if child.parent != *inode {
                    issues.push(format!(
                        "Child/parent mismatch: inode {} thinks parent is {} but {} claims it as child",
                        child_inode, child.parent, inode
                    ));
                }
            } else {
                issues.push(format!(
                    "Directory {} references non-existent child inode {}", 
                    inode, child_inode
                ));
            }
        }
    }
    
    for (inode, file) in &fs.files {
        if let Some(data) = &file.data {
            if file.attrs.size != data.len() as u64 {
                warnings.push(format!(
                    "Size mismatch for '{}' (inode {}): metadata says {} bytes, actual data is {} bytes",
                    fixed_name_to_str(&file.name), inode, file.attrs.size, data.len()
                ));
            }
        } else if file.attrs.size > 0 {
            warnings.push(format!(
                "File '{}' (inode {}) has size {} but no data",
                fixed_name_to_str(&file.name), inode, file.attrs.size
            ));
        }
    }
    
    println!("\n[3/4] Summary of checks:");
    println!("  Total files/directories: {}", fs.files.len());
    println!("  Orphaned files: {}", orphan_count);
    println!("  Cycles detected: {}", if has_cycle { "Yes" } else { "No" });
    println!("  Warnings: {}", warnings.len());
    println!("  Issues: {}", issues.len());
    
    println!("\n[4/4] Detailed report:");
    
    if !warnings.is_empty() {
        println!("\n️  Warnings (non-critical):");
        for warning in &warnings {
            println!("  - {}", warning);
        }
    }
    
    if !issues.is_empty() {
        println!("\n✗ Issues found:");
        for issue in &issues {
            println!("  - {}", issue);
        }
        
        println!("\n=== Filesystem has {} issue(s) to fix ===", issues.len());
        println!("\nRecommendations:");
        println!("1. Consider recreating the filesystem from a known good backup");
        println!("2. Use 'qrfs-mount {} /mnt/temp' to inspect and fix", qr_directory);
        println!("3. Export to new QR codes after fixing: qrfs-export <disk> <new_dir> <passphrase>");
    } else {
        println!("\n✓ All checks passed!");
        println!("✓ Filesystem is consistent and healthy");
        
        let dir_count = fs.files.values().filter(|f| f.attrs.kind == qrfs::FileType::Directory).count();
        let file_count = fs.files.len() - dir_count;
        let total_size: u64 = fs.files.values().map(|f| f.attrs.size).sum();
        
        println!("\n Filesystem statistics:");
        println!("  Directories: {}", dir_count);
        println!("  Files: {}", file_count);
        println!("  Total size: {} bytes", total_size);
        println!("  Inode counter: {}", INODE_COUNTER.load(Ordering::Relaxed));
        
        if dir_count > 0 {
            println!("\n Directory structure preview:");
            print_directory_tree(fs, 1, 0, 2); // Start from root, depth 0, max depth 2
        }
    }
}

fn print_directory_tree(fs: &QRFileSystem, inode: u64, depth: usize, max_depth: usize) {
    if depth > max_depth {
        return;
    }
    
    if let Some(file) = fs.files.get(&inode) {
        let indent = "  ".repeat(depth);
        let name = fixed_name_to_str(&file.name);
        let symbol = if file.attrs.kind == qrfs::FileType::Directory { "d:" } else { "f:" };
        
        println!("{}{} {}", indent, symbol, name);
        
        if file.attrs.kind == qrfs::FileType::Directory && depth < max_depth {
            for child_inode in &file.children {
                print_directory_tree(fs, *child_inode, depth + 1, max_depth);
            }
        }
    }
}