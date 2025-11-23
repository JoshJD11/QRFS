use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::env;
use std::time::Duration;
use std::ffi::OsStr;
use fuser::{FileAttr, FileType, Filesystem, Request, ReplyDirectory, ReplyAttr, ReplyData, ReplyEntry, ReplyEmpty};
use libc::{ENOENT};
use qrcode::QrCode;
use image::Luma;
use data_encoding::BASE64;
use std::fs;

static INODE_COUNTER: AtomicU64 = AtomicU64::new(2); // Start from 2, 1 is reserved for root

use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
enum FileData {
    Text(String),
    Binary(Vec<u8>),
}

impl FileData {
    fn as_bytes(&self) -> Vec<u8> {
        match self {
            FileData::Text(s) => s.as_bytes().to_vec(),
            FileData::Binary(b) => b.clone(),
        }
    }
    
    fn size(&self) -> u64 {
        self.as_bytes().len() as u64
    }
}

// Simplified: Only store file names and their QR block assignments, we could add FileAttrs to FileEntry if needed (later)
#[derive(Serialize, Deserialize, Debug)]
struct FilesystemMetadata {
    pub version: u32,
    pub files: Vec<FileEntry>,
}

#[derive(Serialize, Deserialize, Debug)]
struct FileEntry {
    pub name: String,
    pub qr_blocks: Vec<u32>,
}

struct File {
    pub inode: u64,
    pub name: String,
    pub data: Option<FileData>,
    pub attrs: FileAttr,
}

impl File {
    pub fn new(file_name: String, data: Option<FileData>) -> Self {
        let id: u64 = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);

        let size = data.as_ref().map(|d| d.size()).unwrap_or(0);

        let attr = FileAttr {
            ino: id,
            size,
            blocks: 0,
            atime: std::time::SystemTime::now(),
            mtime: std::time::SystemTime::now(),
            ctime: std::time::SystemTime::now(),
            crtime: std::time::SystemTime::now(),
            kind: FileType::RegularFile,
            perm: 0o644,
            nlink: 0,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 4096,
        };

        Self {
            inode: id,
            name: file_name,
            data,
            attrs: attr,
        }
    }
}

struct QRFileSystem {
    pub root_inode: u64,
    pub files: HashMap<u64, File>, // All files (root is separate)
}

impl QRFileSystem {
    pub fn new() -> Self {
        Self {
            root_inode: 1,
            files: HashMap::new(),
        }
    }

    pub fn add_file(&mut self, file_name: String, data: Option<FileData>) -> u64 {
        let file = File::new(file_name, data);
        let inode = file.inode;
        self.files.insert(inode, file);
        inode
    }

    pub fn remove_file(&mut self, inode: u64) {
        self.files.remove(&inode);
    }

    pub fn binary_to_qr(&self, binary_data: &[u8], output_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let base64_data = BASE64.encode(binary_data);
        
        let code = QrCode::new(base64_data.as_bytes())?;
        
        let image = code.render::<Luma<u8>>()
            .min_dimensions(200, 200)
            .max_dimensions(200, 200)
            .build();
            
        image.save(output_path)?;
        Ok(())
    }
    
    pub fn qr_to_binary(&self, qr_path: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let img = image::open(qr_path)?;
        let luma_img = img.to_luma8();
        
        let mut img_data = rqrr::PreparedImage::prepare(luma_img);
        let grids = img_data.detect_grids();
        
        if grids.is_empty() {
            return Err("No QR code found in image".into());
        }
        
        let (_meta, content) = grids[0].decode()?;
        
        let binary_data = BASE64.decode(content.as_bytes())?;
        
        Ok(binary_data)
    }
    
    fn split_data_for_qr(&self, data: &[u8]) -> Vec<Vec<u8>> {
        const MAX_QR_DATA_SIZE: usize = 1500;
        
        let mut chunks = Vec::new();
        let mut remaining = data;
        
        while !remaining.is_empty() {
            let chunk_size = std::cmp::min(MAX_QR_DATA_SIZE, remaining.len());
            chunks.push(remaining[..chunk_size].to_vec());
            remaining = &remaining[chunk_size..];
        }
        
        chunks
    }
    
    pub fn export_files_as_qr(&self, output_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(output_dir)?;
        
        println!("Exporting filesystem structure...");
        
        let mut metadata = FilesystemMetadata {
            version: 1,
            files: Vec::new(),
        };
        
        for (_inode, file) in &self.files {
            let entry = FileEntry {
                name: file.name.clone(),
                qr_blocks: Vec::new(), // Will be filled when we export data
            };
            metadata.files.push(entry);
            println!("  - {}", file.name);
        }
        
        // Lowkey unnecessary step, but keeping for clarity
        // let metadata_json = serde_json::to_string(&metadata)?;
        // println!("Metadata size: {} bytes", metadata_json.len());
        
        // let qr_path = format!("{}/directory.png", output_dir);
        // self.binary_to_qr(metadata_json.as_bytes(), &qr_path)?;
        // println!("Created directory QR: {}", qr_path);
        
        // later the implementation will change to support a directory that needs multiple blocks
        let mut current_block = 1; // Start from 1 (0 is directory)
        
        for file_entry in &mut metadata.files {
            if let Some(file) = self.files.values().find(|f| f.name == file_entry.name) {
                if let Some(file_data) = &file.data {
                    let data_bytes = file_data.as_bytes();
                    let data_chunks = self.split_data_for_qr(&data_bytes);
                    
                    println!("Exporting file '{}' as {} QR blocks...", file_entry.name, data_chunks.len());
                    
                    for chunk in data_chunks.iter() {
                        let qr_path = format!("{}/file_{:03}.png", output_dir, current_block);
                        self.binary_to_qr(chunk, &qr_path)?;
                        file_entry.qr_blocks.push(current_block);
                        current_block += 1;
                    }
                }
            }
        }
        
        let updated_metadata_json = serde_json::to_string(&metadata)?;
        let qr_path = format!("{}/directory.png", output_dir);
        self.binary_to_qr(updated_metadata_json.as_bytes(), &qr_path)?;
        println!("Updated directory QR with block assignments");
        
        println!("Export completed! Total files: {}, Total QR blocks: {}", 
                 metadata.files.len(), current_block);
        Ok(())
    }
    
    pub fn import_files_from_qr(&mut self, input_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
        println!("Importing from QR codes in: {}", input_dir);
        
        // Read directory QR (later it could be multiple blocks)
        let dir_qr_path = format!("{}/directory.png", input_dir);
        println!("Reading directory QR: {}", dir_qr_path);
        
        let metadata_bytes = self.qr_to_binary(&dir_qr_path)?;
        let metadata: FilesystemMetadata = serde_json::from_slice(&metadata_bytes)
            .map_err(|e| format!("Failed to parse metadata: {}", e))?;
        
        println!("Found {} files in directory", metadata.files.len());
        
        //  Clear existing files and reset counter
        self.files.clear();
        INODE_COUNTER.store(2, Ordering::Relaxed); // Reset to 2 (root is 1)
        
        for file_entry in &metadata.files {
            if !file_entry.qr_blocks.is_empty() {
                let mut file_data = Vec::new();
                
                for &block_num in &file_entry.qr_blocks {
                    // if directory takes multiple blocks, it can be handled as block_num + offset (amount of directory blocks)
                    let qr_path = format!("{}/file_{:03}.png", input_dir, block_num);
                    if let Ok(chunk_data) = self.qr_to_binary(&qr_path) {
                        file_data.extend_from_slice(&chunk_data);
                    } else {
                        eprintln!("Failed to decode data block {}", block_num);
                    }
                }
                
                let file_data_enum = self.detect_data_type(&file_data, &file_entry.name);
                self.add_file(file_entry.name.clone(), Some(file_data_enum));
                
                println!("Imported file: '{}' ({} bytes)", file_entry.name, file_data.len());
            }
        }
        
        println!("\n=== Final Imported Structure ===");
        println!("Root (inode 1) contains:");
        for (inode, file) in &self.files {
            println!("  - {}: '{}' ({} bytes)", inode, file.name, file.attrs.size);
        }
        
        Ok(())
    }
    
    fn detect_data_type(&self, data: &[u8], filename: &str) -> FileData {
        // lets just treat all files as binary for simplicity
        // if let Ok(text) = String::from_utf8(data.to_vec()) {
        //     FileData::Text(text)
        // } else {
            FileData::Binary(data.to_vec())
        // }
    }

    fn find_file_by_name(&self, name: &str) -> Option<&File> {
        self.files.values().find(|f| f.name == name)
    }

    // For debugging
    fn get_file_inodes(&self) -> Vec<u64> {
        self.files.keys().cloned().collect()
    }

}

impl Filesystem for QRFileSystem {
    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        if ino == self.root_inode {
            // Root directory attributes
            let attr = FileAttr {
                ino: self.root_inode,
                size: 0,
                blocks: 0,
                atime: std::time::SystemTime::now(),
                mtime: std::time::SystemTime::now(),
                ctime: std::time::SystemTime::now(),
                crtime: std::time::SystemTime::now(),
                kind: FileType::Directory,
                perm: 0o755,
                nlink: 0,
                uid: 0,
                gid: 0,
                rdev: 0,
                flags: 0,
                blksize: 4096,
            };
            let ttl = Duration::from_secs(1);
            reply.attr(&ttl, &attr);
        } else if let Some(file) = self.files.get(&ino) {
            // File attributes
            let ttl = Duration::from_secs(1);
            reply.attr(&ttl, &file.attrs);
        } else {
            reply.error(ENOENT);
        }
    }

    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if parent != self.root_inode {
            reply.error(ENOENT);
            return;
        }

        let name_str = name.to_str().unwrap();

        if let Some(file) = self.find_file_by_name(name_str) {
            let ttl = Duration::from_secs(1);
            reply.entry(&ttl, &file.attrs, 0);
        } else {
            reply.error(ENOENT);
        }
    }

    fn read(&mut self, _req: &Request, ino: u64, _fh: u64, _offset: i64, _size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        if let Some(file) = self.files.get(&ino) {
            if let Some(data) = &file.data {
                reply.data(&data.as_bytes());
            } else {
                reply.data(&[]);
            }
        } else {
            reply.error(ENOENT);
        }
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        if ino != self.root_inode {
            reply.error(ENOENT);
            return;
        }

        let entries = vec![
            (self.root_inode, ".", FileType::Directory),
            (self.root_inode, "..", FileType::Directory),
        ];

        let file_entries: Vec<_> = self.files.iter()
            .map(|(inode, file)| (*inode, file.name.as_str(), FileType::RegularFile))
            .collect();

        // Add all entries
        for (i, (inode, name, file_type)) in entries.iter().chain(file_entries.iter()).enumerate() {
            if i as i64 >= offset {
                if reply.add(*inode, (i + 1) as i64, *file_type, name) {
                    break;
                }
            }
        }

        reply.ok();
    }

    // Stub implementations for other required methods (just to test read-only FS)
    fn rename(&mut self, _req: &Request, _parent: u64, _name: &OsStr, _newparent: u64, _newname: &OsStr, _flags: u32, reply: ReplyEmpty) {
        reply.error(libc::EROFS); // Read-only filesystem
    }

    fn rmdir(&mut self, _req: &Request, _parent: u64, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(libc::EROFS); // Read-only filesystem
    }
}

fn main() {
    println!("=== Simplified QR Filesystem Test ===");
    
    let mut original_fs = QRFileSystem::new();
    
    println!("\n1. Creating test filesystem structure...");
    
    original_fs.add_file("readme.txt".to_string(), 
                     Some(FileData::Text("Welcome to QR Filesystem!\nThis is a test file.".to_string())));
    
    original_fs.add_file("note.txt".to_string(), 
                     Some(FileData::Text("This is a simple note.".to_string())));
    
    original_fs.add_file("config.json".to_string(), 
                     Some(FileData::Text(r#"{"version": "1.0", "author": "QR FS"}"#.to_string())));
    
    original_fs.add_file("data.bin".to_string(), 
                     Some(FileData::Binary(vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05])));

    println!("\nOriginal structure:");
    for (inode, file) in &original_fs.files {
        println!("  {}: '{}' ({} bytes)", inode, file.name, file.attrs.size);
    }

    let test_dir = "./qr_test_simple";
    
    let _ = fs::remove_dir_all(test_dir);

    println!("\n2. Exporting to QR codes...");
    if let Err(e) = original_fs.export_files_as_qr(test_dir) {
        eprintln!("Export failed: {}", e);
        return;
    }

    println!("\n3. Importing from QR codes...");
    let mut imported_fs = QRFileSystem::new();
    if let Err(e) = imported_fs.import_files_from_qr(test_dir) {
        eprintln!("Import failed: {}", e);
        return;
    }

    println!("\n4. Mounting filesystem...");
    let mountpoint = match env::args().nth(1) {
        Some(p) => p,
        None => {
            println!("Usage: {} <MOUNTPOINT>", env::args().next().unwrap());
            return;
        }
    };

    println!("Mounting at: {}", mountpoint);
    match fuser::mount2(imported_fs, &mountpoint, &[]) {
        Ok(_) => {
            println!("Mounted successfully!");
            println!("Try: ls -la {}", mountpoint);
            println!("Try: cat {}/readme.txt", mountpoint);
            println!("Try: cat {}/config.json", mountpoint);
            println!("\nUse 'fusermount -u {}' to unmount", mountpoint);
        },
        Err(e) => println!("Mount failed: {:?}", e),
    }
}