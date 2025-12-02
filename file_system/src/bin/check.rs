use qrfs::{QRFileSystem, initialize_new_disk};
use std::env;
use std::path::Path;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("=== QR Filesystem Consistency Check ===");
        println!("Usage:");
        println!("  {} <disk_path>", args[0]);
        println!("\nExamples:");
        println!("  {} my_fs.bin", args[0]);
        return Ok(());
    }
    
    let disk_path = &args[1];
    
    if !Path::new(disk_path).exists() {
        eprintln!("Error: Disk file '{}' does not exist", disk_path);
        return Ok(());
    }
    
    println!("Checking filesystem consistency for: {}", disk_path);
    
    let mut fs = QRFileSystem::new(disk_path, "null");
    
    match fs.load_fs_from_disk() {
        Ok(_) => {
            println!("✓ Disk file structure is valid");
            println!("✓ Loaded {} files/directories", fs.files.len());
            
            let mut orphan_count = 0;
            for (inode, file) in &fs.files {
                if file.parent != 0 && !fs.files.contains_key(&file.parent) {
                    println!("✗ Orphaned file: inode {} ('{}') has non-existent parent {}", 
                            inode, qrfs::fixed_name_to_str(&file.name), file.parent);
                    orphan_count += 1;
                }
            }
            
            let mut visited = std::collections::HashSet::new();
            let mut has_cycle = false;
            
            for (inode, file) in &fs.files {
                if !visited.contains(inode) {
                    let mut current = inode;
                    let mut path = vec![*current];
                    
                    while *current != 0 {
                        if let Some(parent_file) = fs.files.get(current) {
                            current = &parent_file.parent;
                            if path.contains(current) {
                                println!("✗ Cycle detected in filesystem hierarchy");
                                has_cycle = true;
                                break;
                            }
                            path.push(*current);
                        } else {
                            break;
                        }
                    }
                    visited.extend(path);
                }
            }
            
            let mut missing_blocks = 0;
            for (inode, block) in &fs.inode_block_table {
                if !fs.files.contains_key(inode) {
                    println!("✗ Block {} maps to non-existent inode {}", block, inode);
                    missing_blocks += 1;
                }
            }
            
            if orphan_count == 0 && !has_cycle && missing_blocks == 0 {
                println!("✓ Filesystem hierarchy is consistent");
                println!("✓ No orphaned files found");
                println!("✓ No cycles detected");
                println!("✓ Inode-block mapping is valid");
                println!("\n=== All checks passed! ===");
            } else {
                println!("\n=== Found {} issue(s) ===", orphan_count + (if has_cycle { 1 } else { 0 }) + missing_blocks);
            }
        }
        Err(e) => {
            eprintln!("✗ Failed to load filesystem: {}", e);
            println!("\n=== Disk file is corrupted or invalid ===");
        }
    }
    
    Ok(())
}