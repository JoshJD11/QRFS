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
use std::io;

static INODE_COUNTER: AtomicU64 = AtomicU64::new(1);

// Support different data types (Josh might need to cook here)
#[derive(Clone)]
enum FileData {
    Text(String),
    Binary(Vec<u8>),
    Json(serde_json::Value), // need serde_json for this
}

impl FileData {
    fn as_bytes(&self) -> Vec<u8> {
        match self {
            FileData::Text(s) => s.as_bytes().to_vec(),
            FileData::Binary(b) => b.clone(),
            FileData::Json(v) => v.to_string().into_bytes(),
        }
    }
    
    fn size(&self) -> u64 {
        self.as_bytes().len() as u64
    }
}

struct File {
    pub inode: u64,
    pub name: String,
    pub data: Option<FileData>,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub attrs: FileAttr,
}

impl File {
    pub fn new(file_name: String, data: Option<FileData>, parent_inode: Option<u64>, folder_flag: bool) -> Self {
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
            kind: if folder_flag {
                FileType::Directory
            } else {
                FileType::RegularFile
            },
            perm: 0o755,
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
            parent: parent_inode,
            children: Vec::new(),
            attrs: attr,
        }
    }
}

struct QRFileSystem {
    pub files: HashMap<u64, File>,
}

impl QRFileSystem {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    pub fn push(&mut self, file_name: String, data: Option<FileData>, parent_inode: Option<u64>, folder_flag: bool) {
        let file: File = File::new(file_name, data, parent_inode, folder_flag);
        let inode = file.inode;
        self.files.insert(inode, file);

        if let Some(pid) = parent_inode {
            if let Some(parent) = self.files.get_mut(&pid) {
                parent.children.push(inode);
            }
        }
    }

    pub fn pop_recursively(&mut self, inode: u64) {
        if let Some(file) = self.files.get(&inode) {
            let children = file.children.clone();
            for child_inode in children {
                self.pop_recursively(child_inode);
            }
        }
        self.files.remove(&inode);
    }

    pub fn delete_file(&mut self, parent_inode: u64, file_name: String) {
        let mut target_inode: Option<u64> = None;

        if let Some(parent_file) = self.files.get(&parent_inode) {
            for &child_inode in &parent_file.children {
                if let Some(child) = self.files.get(&child_inode) {
                    if child.name == file_name {
                        target_inode = Some(child_inode);
                        break;
                    }
                }
            }
        }

        let target_inode = match target_inode {
            Some(i) => i,
            None => return,
        };

        if let Some(parent_file) = self.files.get_mut(&parent_inode) {
            parent_file.children.retain(|&x| x != target_inode);
        }

        self.pop_recursively(target_inode);
    }

    pub fn rename(&mut self, old_parent_inode: u64, file_old_name: String, new_parent_inode: u64, file_new_name: String) {
        let mut found_child_inode: Option<u64> = None;

        if let Some(parent_file) = self.files.get(&old_parent_inode) {
            for &child_inode in &parent_file.children {
                if let Some(child) = self.files.get(&child_inode) {
                    if child.name == file_old_name {
                        found_child_inode = Some(child_inode);
                        break;
                    }
                }
            }
        }

        let child_inode = match found_child_inode {
            Some(i) => i,
            None => return,
        };

        if let Some(child) = self.files.get_mut(&child_inode) {
            child.name = file_new_name;
            child.parent = Some(new_parent_inode);
        }

        if let Some(parent_file) = self.files.get_mut(&old_parent_inode) {
            parent_file.children.retain(|&x| x != child_inode);
        }

        if let Some(new_parent) = self.files.get_mut(&new_parent_inode) {
            new_parent.children.push(child_inode);
        }
    }

    pub fn binary_to_qr(&self, binary_data: &[u8], output_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let base64_data = BASE64.encode(binary_data);
        
        let code = QrCode::new(base64_data.as_bytes())?;
        
        // Render as 200x200 pixel image
        let image = code.render::<Luma<u8>>()
            .min_dimensions(200, 200)
            .max_dimensions(200, 200)
            .build();
            
        image.save(output_path)?;
        println!("QR code saved to: {}", output_path);
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
    
    pub fn test_qr_roundtrip(&self, test_data: &[u8]) -> Result<bool, Box<dyn std::error::Error>> {
        let temp_path = "./test_qr_roundtrip.png";
        self.binary_to_qr(test_data, temp_path)?;
        let decoded_data = self.qr_to_binary(temp_path)?;
        let _ = fs::remove_file(temp_path);
        
        Ok(test_data == decoded_data.as_slice())
    }
    
    pub fn export_files_as_qr(&self, output_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(output_dir)?;
        
        for (inode, file) in &self.files {
            if file.attrs.kind == FileType::RegularFile {
                if let Some(content) = &file.data {
                    let binary_data = content.as_bytes();
                    let sanitized_name = file.name.replace("/", "_").replace("..", "_"); // Simple sanitization to avoid path traversal
                    let qr_path = format!("{}/file_{}_{}.png", output_dir, inode, sanitized_name);
                    
                    self.binary_to_qr(&binary_data, &qr_path)?;
                    println!("Exported '{}' as QR code: {}", file.name, qr_path);
                }
            }
        }
        Ok(())
    }
    
    pub fn import_files_from_qr(&mut self, input_dir: &str, parent_inode: u64) -> Result<(), Box<dyn std::error::Error>> {
        let entries = fs::read_dir(input_dir)?;
        
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("png") {
                match self.qr_to_binary(path.to_str().unwrap()) {
                    Ok(binary_data) => {
                        // Try to detect data type (this might need an upgrade later)
                        let file_data = self.detect_data_type(&binary_data);
                        let file_name = path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("imported_file")
                            .to_string();
                        
                        self.push(file_name, Some(file_data), Some(parent_inode), false);
                        println!("Successfully imported file from QR: {}", path.display());
                    }
                    Err(e) => {
                        eprintln!("Failed to decode QR {}: {}", path.display(), e);
                    }
                }
            }
        }
        Ok(())
    }
    
    // Detect data type and create appropriate FileData (Josh might need to cook here)
    fn detect_data_type(&self, data: &[u8]) -> FileData {
        // Try to parse as UTF-8 text
        if let Ok(text) = String::from_utf8(data.to_vec()) {
            // Try to parse as JSON
            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&text) {
                return FileData::Json(json_value);
            }
            return FileData::Text(text);
        }
        
        // If not valid UTF-8, treat as binary
        FileData::Binary(data.to_vec())
    }
    
    // test function
    pub fn run_comprehensive_tests(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Running comprehensive QR tests...");
        
        let text_data = b"Hello, QR World!";
        println!("Test 1 - Text data: {}", self.test_qr_roundtrip(text_data)?);
        
        let binary_data = vec![0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE, 0xFD];
        println!("Test 2 - Binary data: {}", self.test_qr_roundtrip(&binary_data)?);
        
        let json_data = br#"{"name": "test", "value": 42, "active": true}"#;
        println!("Test 3 - JSON data: {}", self.test_qr_roundtrip(json_data)?);
        
        let empty_data = b"";
        println!("Test 4 - Empty data: {}", self.test_qr_roundtrip(empty_data)?);
        
        let large_data = vec![0xAB; 1000]; // 1000 bytes
        println!("Test 5 - Large data: {}", self.test_qr_roundtrip(&large_data)?);
        
        Ok(())
    }
}

impl Filesystem for QRFileSystem {
    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        match self.files.get(&ino) {
            Some(file) => {
                let attr = &file.attrs;
                let ttl = Duration::from_secs(1);
                reply.attr(&ttl, attr);
            },
            None => reply.error(ENOENT),
        }
    }

    fn rename(&mut self, _req: &Request, parent: u64, name: &OsStr, newparent: u64, newname: &OsStr, _flags: u32, reply: ReplyEmpty) {
        let old_name = name.to_str().unwrap().to_string();
        let new_name = newname.to_str().unwrap().to_string();
        self.rename(parent, old_name, newparent, new_name);
        reply.ok();
    }

    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let file_name = name.to_str().unwrap().to_string();
        self.delete_file(parent, file_name);
        reply.ok();
    }

    fn read(&mut self, _req: &Request, ino: u64, _fh: u64, _offset: i64, _size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        let file = match self.files.get(&ino) {
            Some(f) => f,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        if file.attrs.kind == FileType::Directory {
            reply.error(ENOENT);
            return;
        }

        let data = match &file.data {
            Some(d) => d.as_bytes(),
            None => {
                reply.data(&[]);
                return;
            }
        };

        reply.data(&data);
    }

    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_str().unwrap().to_string();

        let parent_file = match self.files.get(&parent) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let mut found_inode: Option<u64> = None;

        for &child_inode in &parent_file.children {
            if let Some(child) = self.files.get(&child_inode) {
                if child.name == name_str {
                    found_inode = Some(child_inode);
                    break;
                }
            }
        }

        let inode = match found_inode {
            Some(i) => i,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let file = self.files.get(&inode).unwrap();
        let attr = &file.attrs;

        let ttl = Duration::from_secs(1);
        reply.entry(&ttl, attr, 0);
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        let dir = match self.files.get(&ino) {
            Some(f) => f,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        if dir.attrs.kind != FileType::Directory {
            reply.error(ENOENT);
            return;
        }

        if offset == 0 {
            let _ = reply.add(ino, 1, FileType::Directory, ".");
            let parent = dir.parent.unwrap_or(ino);
            let _ = reply.add(parent, 2, FileType::Directory, "..");
        }

        let mut index = offset - 2;
        if index < 0 { index = 0; }

        let children = &dir.children;

        for (i, &child_inode) in children.iter().enumerate().skip(index as usize) {
            if let Some(child) = self.files.get(&child_inode) {
                let next_offset = 3 + i as i64;
                let _ = reply.add(child.inode, next_offset, child.attrs.kind, child.name.as_str());
            }
        }

        reply.ok();
    }
}

fn main() {
    let mut fs = QRFileSystem::new();

    // Create initial file structure with different data types
    fs.push("/".to_string(), None, None, true);
    fs.push("text_file.txt".to_string(), Some(FileData::Text("Hello, World!\n".to_string())), Some(1), false);
    fs.push("binary_file.bin".to_string(), Some(FileData::Binary(vec![0x00, 0x01, 0x02, 0x03, 0xFF])), Some(1), false);
    
    // Run comprehensive tests first
    println!("=== Running QR Code Tests ===");
    if let Err(e) = fs.run_comprehensive_tests() {
        eprintln!("Test failed: {}", e);
    }

    println!("\n=== Exporting Files as QR Codes ===");
    let test_dir = "./qr_test";
    if let Err(e) = fs.export_files_as_qr(test_dir) {
        eprintln!("QR export failed: {}", e);
    }

    println!("\n=== Importing Files from QR Codes ===");
    if let Err(e) = fs.import_files_from_qr(test_dir, 1) {
        eprintln!("QR import failed: {}", e);
    }

    let mountpoint = match env::args().nth(1) {
        Some(p) => p,
        None => {
            println!("Usage: <program> <MOUNTPOINT>");
            return;
        }
    };

    println!("\n=== Mounting Filesystem at: {} ===", mountpoint);
    match fuser::mount2(fs, &mountpoint, &[]) {
        Ok(_) => println!("Mounted successfully"),
        Err(e) => println!("ERROR MOUNTING: {:?}", e),
    }
}

// Note: if you cloned the repository, you have to make a dir to mount the fs out of the repo
// run with cargo run -- ~/Desktop/fs
// To unmount, run the command: fusermount -u ~/Desktop/fs
// if you don't unmount, you'll run into errors next time you try cargo run.
// TO RUN THE PROGRAM YOU HAVE TO USE OTHER TERMINAL, DO NOT USE THE VS CODE TERMINAL.
