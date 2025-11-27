use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::env;
use std::time::Duration;
use std::ffi::OsStr;
use fuser::{FileAttr, FileType, Filesystem, Request, ReplyDirectory, ReplyAttr, ReplyData, ReplyEntry, ReplyEmpty, ReplyOpen, ReplyCreate, ReplyWrite};
use libc::{ENOENT};
use std::time::SystemTime;
use qrcode::QrCode;
use image::Luma;
use data_encoding::BASE64;
use std::fs;
use serde::{Serialize, Deserialize};

static INODE_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Serialize, Deserialize, Debug)]
pub struct FilesystemMetadata {
    pub version: u32,
    pub files: Vec<FileEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase_hash: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct FileEntry {
    pub name: String,
    pub qr_blocks: Vec<u32>,
    pub parent: Option<u64>,
    pub is_directory: bool,
}

struct File {
    pub inode: u64,
    pub name: String,
    pub data: Option<Vec<u8>>,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub attrs: FileAttr,
}

impl File {
    pub fn new(file_name: String, file_data: Option<Vec<u8>>, parent_inode: Option<u64>, folder_flag: bool) -> Self {
        let id: u64 = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);

        let size = match &file_data {
            Some(v) => v.len() as u64,
            None => 0,
        };

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

        Self{
            inode: id,
            name: file_name,
            data: file_data,
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

    pub fn push(&mut self, file_name: String, data: Option<Vec<u8>>, parent_inode: Option<u64>, folder_flag: bool) {
        let file: File = File::new(file_name, data, parent_inode, folder_flag);
        let inode = file.inode;
        self.files.insert(inode, file);

        if let Some(pid) = parent_inode {
            if let Some(parent) = self.files.get_mut(&pid) {
                parent.children.push(inode);
            }
        }
    }

    pub fn rename(&mut self, old_parent_inode: u64, file_old_name: String, new_parent_inode: u64, file_new_name: String,) {
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

    fn hash_passphrase(&self, passphrase: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        passphrase.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
    
    pub fn export_files_as_qr(&self, output_dir: &str, passphrase: &str) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(output_dir)?;
        
        println!("Exporting filesystem structure with passphrase protection...");
        
        let mut metadata = FilesystemMetadata {
            version: 1,
            files: Vec::new(),
            passphrase_hash: Some(self.hash_passphrase(passphrase)),
        };
        
        for (_inode, file) in &self.files {
            let chunk_count = if let Some(file_data) = &file.data {
                let data_chunks = self.split_data_for_qr(&file_data);
                data_chunks.len()
            } else {
                0
            };
            
            let entry = FileEntry {
                name: file.name.clone(),
                qr_blocks: vec![0; chunk_count],
                parent: file.parent,
                is_directory: file.attrs.kind == FileType::Directory,
            };
            metadata.files.push(entry);
            println!("  - {} ({} chunks)", file.name, chunk_count);
        }
        
        let mut current_block = 0;
        
        let metadata_json = serde_json::to_string(&metadata)?;
        println!("Initial metadata size: {} bytes", metadata_json.len());
        
        let metadata_chunks = self.split_data_for_qr(metadata_json.as_bytes());
        println!("Directory metadata requires {} QR blocks", metadata_chunks.len());
        
        for (chunk_index, chunk) in metadata_chunks.iter().enumerate() {
            let qr_path = format!("{}/file_{:03}.png", output_dir, current_block);
            self.binary_to_qr(chunk, &qr_path)?;
            println!("  Created directory block {}: {}", chunk_index, qr_path);
            current_block += 1;
        }
        
        let directory_blocks_count = metadata_chunks.len() as u32;
        
        for file_entry in &mut metadata.files {
            if let Some(file) = self.files.values().find(|f| f.name == file_entry.name) {
                if let Some(file_data) = &file.data {
                    let data_chunks = self.split_data_for_qr(&file_data);
                    
                    println!("Exporting file '{}' as {} QR blocks...", file_entry.name, data_chunks.len());
                    
                    for (chunk_index, chunk) in data_chunks.iter().enumerate() {
                        let qr_path = format!("{}/file_{:03}.png", output_dir, current_block);
                        self.binary_to_qr(chunk, &qr_path)?;
                        file_entry.qr_blocks[chunk_index] = current_block;
                        println!("  Created file block {}: {}", current_block, qr_path);
                        current_block += 1;
                    }
                }
            }
        }
        
        let final_metadata_json = serde_json::to_string(&metadata)?;
        
        let mut final_metadata_with_passphrase = final_metadata_json.clone();
        final_metadata_with_passphrase.push_str(&format!("|PASSPHRASE:{}", passphrase));
        
        let final_metadata_chunks = self.split_data_for_qr(final_metadata_with_passphrase.as_bytes());
        
        for (chunk_index, chunk) in final_metadata_chunks.iter().enumerate() {
            if chunk_index < directory_blocks_count as usize {
                let qr_path = format!("{}/file_{:03}.png", output_dir, chunk_index as u32);
                self.binary_to_qr(chunk, &qr_path)?;
                println!("  Updated directory block {} with final metadata", chunk_index);
            } else {
                let qr_path = format!("{}/file_{:03}.png", output_dir, current_block);
                self.binary_to_qr(chunk, &qr_path)?;
                println!("  Added directory block {}: {}", current_block, qr_path);
                current_block += 1;
            }
        }
        
        println!("Export completed! Total files: {}, Total QR blocks: {}", 
                metadata.files.len(), current_block);
        println!("Passphrase protection enabled. Remember your passphrase: '{}'", passphrase);
        
        Ok(())
    }
    
    pub fn import_files_from_qr(&mut self, input_dir: &str, expected_passphrase: &str) -> Result<(), Box<dyn std::error::Error>> {
        println!("Importing from QR codes in: {}", input_dir);
        println!("Verifying passphrase...");
        
        let expected_hash = self.hash_passphrase(expected_passphrase);
        
        let mut directory_blocks = Vec::new();
        let mut current_block = 0;
        let mut found_passphrase = false;
        let mut final_metadata = None;
        
        loop {
            let qr_path = format!("{}/file_{:03}.png", input_dir, current_block);
            if !std::path::Path::new(&qr_path).exists() {
                break;
            }
            
            match self.qr_to_binary(&qr_path) {
                Ok(data) => {
                    directory_blocks.push(data);
                    current_block += 1;
                    
                    let combined_data: Vec<u8> = directory_blocks.iter().flatten().cloned().collect();
                    if let Ok(combined_str) = String::from_utf8(combined_data.clone()) {
                        if let Some(passphrase_pos) = combined_str.find("|PASSPHRASE:") {
                            let metadata_str = &combined_str[..passphrase_pos];
                            let actual_passphrase = &combined_str[passphrase_pos + "|PASSPHRASE:".len()..];
                            
                            if actual_passphrase == expected_passphrase {
                                println!("Passphrase verified successfully");
                                found_passphrase = true;
                                
                                match serde_json::from_str::<FilesystemMetadata>(metadata_str) {
                                    Ok(metadata) => {
                                        if let Some(stored_hash) = &metadata.passphrase_hash {
                                            if stored_hash == &expected_hash {
                                                println!("Passphrase hash verified");
                                            } else {
                                                eprintln!("Passphrase hash mismatch (file may be modified)");
                                            }
                                        }
                                        
                                        final_metadata = Some(metadata);
                                        break;
                                    }
                                    Err(e) => {
                                        return Err(format!("Failed to parse metadata: {}", e).into());
                                    }
                                }
                            } else {
                                return Err(format!("Incorrect passphrase. Expected '{}', found '{}'", 
                                                expected_passphrase, actual_passphrase).into());
                            }
                        }
                    }
                }
                Err(e) => {
                    if !found_passphrase {
                        return Err(format!("Failed to decode directory block {}: {}", current_block, e).into());
                    }
                    break;
                }
            }
            
            if current_block > 1000 {
                return Err("Too many directory blocks or corrupted directory".into());
            }
        }
        
        if !found_passphrase {
            return Err("Passphrase delimiter not found. Either wrong passphrase or corrupted filesystem.".into());
        }
        
        let metadata = final_metadata.ok_or("Failed to parse filesystem metadata")?;
        
        println!("Found {} files in directory", metadata.files.len());
        
        self.files.clear();
        INODE_COUNTER.store(1, Ordering::Relaxed);
        
        for file_entry in &metadata.files {
            if file_entry.is_directory {
                self.push(file_entry.name.clone(), None, file_entry.parent, true);
            }
        }
        
        for file_entry in &metadata.files {
            if !file_entry.is_directory && !file_entry.qr_blocks.is_empty() {
                let mut file_data = Vec::new();
                
                for &block_num in &file_entry.qr_blocks {
                    let qr_path = format!("{}/file_{:03}.png", input_dir, block_num);
                    match self.qr_to_binary(&qr_path) {
                        Ok(chunk_data) => {
                            file_data.extend_from_slice(&chunk_data);
                        }
                        Err(e) => {
                            return Err(format!("Failed to decode data block {} for '{}': {}", 
                                            block_num, file_entry.name, e).into());
                        }
                    }
                }
                
                self.push(file_entry.name.clone(), Some(file_data), file_entry.parent, false);
                
                println!("Imported file: '{}' ({} blocks)", 
                        file_entry.name, file_entry.qr_blocks.len());
            }
        }
        
        println!("\n=== Import completed successfully ===");
        println!("Root contains {} files and directories", self.files.len());
        
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

    fn write(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, data: &[u8], _write_flags: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyWrite) {
        let file: &mut File = match self.files.get_mut(&ino) {
            Some(f) => f,
            None => {
                reply.error(ENOENT);
                return ;
            }
        };

        if file.attrs.kind == FileType::Directory {
            reply.error(ENOENT);
            return ;
        }

        if file.data.is_none() {
            file.data = Some(Vec::new());
        }

        let buffer = file.data.as_mut().unwrap();
        let offset = offset as usize;
        let required_size = offset + data.len();

        if buffer.len() < required_size {
            buffer.resize(required_size, 0);
        }

        buffer[offset..offset + data.len()].copy_from_slice(data);
        file.attrs.size = buffer.len() as u64;
        reply.written(data.len() as u32);
    }

    fn access(&mut self, _req: &Request, ino: u64, mask: i32, reply: ReplyEmpty) {
        println!("Calling to access...");

        let file = match self.files.get(&ino) {
            Some(f) => f,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let perm = file.attrs.perm;

        const R_OK: i32 = 4;
        const W_OK: i32 = 2;
        const X_OK: i32 = 1;

        if (mask & R_OK) != 0 && (perm & 0o444 == 0) {
            reply.error(libc::EACCES);
            return;
        }

        if (mask & W_OK) != 0 && (perm & 0o222 == 0) {
            reply.error(libc::EACCES);
            return;
        }

        if (mask & X_OK) != 0 && (perm & 0o111 == 0) {
            reply.error(libc::EACCES);
            return;
        }

        reply.ok();
    }

    fn create(&mut self, _req: &Request, parent: u64, name: &OsStr, _mode: u32, _umask: u32, flags: i32, reply: ReplyCreate) {
        let file_name = name.to_str().unwrap().to_string();
        self.push(file_name, None, Some(parent), false);
        let actual_inode = INODE_COUNTER.load(Ordering::Relaxed) - 1;
        
        match self.files.get(&actual_inode) {
            Some(file) => {
                let attr = &file.attrs;
                let ttl = Duration::from_secs(1);
                reply.created(&ttl, attr, 0, 0, flags.try_into().unwrap());
            },
            None => reply.error(ENOENT),
        }
    }

    fn open(&mut self, _req: &Request, ino: u64, flags: i32, reply: ReplyOpen) {
        let file = match self.files.get(&ino) {
            Some(f) => f,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let write_mode = flags & (libc::O_WRONLY | libc::O_RDWR) != 0;
        if file.attrs.kind == FileType::Directory && write_mode {
            reply.error(libc::EISDIR);
            return;
        }

        println!("open called for ino={}", ino);
        let fh = ino;
        reply.opened(fh, 0);
    }

    fn setattr(
        &mut self,
        _req: &Request,
        ino: u64,
        mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<fuser::TimeOrNow>,
        _mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        let file = match self.files.get_mut(&ino) {
            Some(f) => f,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        if let Some(m) = mode {
            file.attrs.perm = (m & 0o777) as u16;
        }

        if let Some(sz) = size {
            if let Some(data) = file.data.as_mut() {
                data.resize(sz as usize, 0);
            }
            file.attrs.size = sz;
        }

        reply.attr(&Duration::new(1, 0), &file.attrs);
    }

    fn mkdir(&mut self, _req: &Request, parent: u64, name: &OsStr, _mode: u32, _umask: u32, reply: ReplyEntry) { 
        let file_name = match name.to_str() {
            Some(n) => n,
            None => {
                reply.error(ENOENT);
                return;
            }
        };
        self.push(file_name.to_string(), None, Some(parent), true);
        let actual_inode = INODE_COUNTER.load(Ordering::Relaxed) - 1;
        let file = self.files.get(&actual_inode).unwrap();

        reply.entry(&Duration::new(1, 0), &file.attrs, 0);
    }

    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let name = match name.to_str() {
            Some(n) => n,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let children = match self.files.get(&parent) {
            Some(f) => f.children.clone(),
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let mut target_inode: Option<u64> = None;

        for child_inode in children {
            if let Some(child) = self.files.get(&child_inode) {
                if child.name == name {
                    if child.attrs.kind != FileType::Directory {
                        reply.error(libc::ENOTDIR);
                        return;
                    }

                    if !child.children.is_empty() {
                        reply.error(libc::ENOTEMPTY);
                        return;
                    }

                    target_inode = Some(child_inode);
                    break;
                }
            }
        }

        let target_inode = match target_inode {
            Some(i) => i,
            None => {
                reply.error(ENOENT);
                return ;
            }
        };

        if let Some(parent_file) = self.files.get_mut(&parent) {
            parent_file.children.retain(|&x| x != target_inode);
        }

        self.files.remove(&target_inode);
        reply.ok();
    }

    fn read(&mut self, _req: &Request, ino: u64, _fh: u64, _offset: i64, _size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        let file = match self.files.get(&ino) {
            Some(f) => f,
            None => {
                reply.error(ENOENT);
                return ;
            }
        };

        if file.attrs.kind == FileType::Directory {
            reply.error(ENOENT);
            return ;
        }

        let data = match &file.data {
            Some(d) => d,
            None => {
                reply.data(&[]);
                return ;
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

    fs.push("/".to_string(), None, None, true);
    fs.push("dirA".to_string(), None, Some(1), true);
    fs.push("fileB.txt".to_string(), Some(b"Contenido B".to_vec()), Some(1), false);
    fs.push("fileC.txt".to_string(), Some(b"Contenido C".to_vec()), Some(1), false);

    let args: Vec<String> = env::args().collect();
    
    if args.len() > 1 && args[1] == "--export" {
        let output_dir = if args.len() > 2 { &args[2] } else { "./qr_export" };
        let passphrase = if args.len() > 3 { &args[3] } else { "default_passphrase" };
        
        println!("Exporting filesystem to QR codes...");
        if let Err(e) = fs.export_files_as_qr(output_dir, passphrase) {
            eprintln!("Export failed: {}", e);
            return;
        }
        println!("Export completed to: {}", output_dir);
        return;
    }
    
    if args.len() > 1 && args[1] == "--import" {
        let input_dir = if args.len() > 2 { &args[2] } else { "./qr_export" };
        let passphrase = if args.len() > 3 { &args[3] } else { "default_passphrase" };
        
        println!("Importing filesystem from QR codes...");
        let mut imported_fs = QRFileSystem::new();
        if let Err(e) = imported_fs.import_files_from_qr(input_dir, passphrase) {
            eprintln!("Import failed: {}", e);
            return;
        }
        
        fs = imported_fs;
        println!("Import completed successfully!");
    }

    let mountpoint = match env::args().nth(if args.len() > 1 && (args[1] == "--export" || args[1] == "--import") { 4 } else { 1 }) {
        Some(p) => p,
        None => {
            println!("Usage:");
            println!("  Mount only: {} <MOUNTPOINT>", args[0]);
            println!("  Export: {} --export [output_dir] [passphrase]", args[0]);
            println!("  Import and mount: {} --import [input_dir] [passphrase] <MOUNTPOINT>", args[0]);
            return;
        }
    };

    println!("Mounting filesystem at: {}", mountpoint);
    match fuser::mount2(fs, &mountpoint, &[]) {
        Ok(_) => println!("Mounted successfully"),
        Err(e) => println!("ERROR MOUNTING: {:?}", e),
    }
}