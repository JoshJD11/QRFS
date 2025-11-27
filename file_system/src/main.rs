use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct SerializableFileAttr {
    pub ino: u64,
    pub size: u64,
    pub blocks: u64,
    pub atime_sec: u64,
    pub atime_nsec: u32,
    pub mtime_sec: u64,
    pub mtime_nsec: u32,
    pub ctime_sec: u64,
    pub ctime_nsec: u32,
    pub crtime_sec: u64,
    pub crtime_nsec: u32,
    pub kind: u32,
    pub perm: u16,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u32,
    pub flags: u32,
    pub blksize: u32,
}

impl SerializableFileAttr {
    pub fn from_file_attr(attr: &FileAttr) -> Self {
        Self {
            ino: attr.ino,
            size: attr.size,
            blocks: attr.blocks,
            atime_sec: attr.atime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            atime_nsec: attr.atime.duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos(),
            mtime_sec: attr.mtime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            mtime_nsec: attr.mtime.duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos(),
            ctime_sec: attr.ctime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            ctime_nsec: attr.ctime.duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos(),
            crtime_sec: attr.crtime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            crtime_nsec: attr.crtime.duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos(),
            kind: match attr.kind {
                FileType::NamedPipe => 1,
                FileType::CharDevice => 2,
                FileType::BlockDevice => 3,
                FileType::Directory => 4,
                FileType::RegularFile => 5,
                FileType::Symlink => 6,
                FileType::Socket => 7,
            },
            perm: attr.perm,
            nlink: attr.nlink,
            uid: attr.uid,
            gid: attr.gid,
            rdev: attr.rdev,
            flags: attr.flags,
            blksize: attr.blksize,
        }
    }

    pub fn to_file_attr(&self) -> FileAttr {
        FileAttr {
            ino: self.ino,
            size: self.size,
            blocks: self.blocks,
            atime: std::time::UNIX_EPOCH + std::time::Duration::new(self.atime_sec, self.atime_nsec),
            mtime: std::time::UNIX_EPOCH + std::time::Duration::new(self.mtime_sec, self.mtime_nsec),
            ctime: std::time::UNIX_EPOCH + std::time::Duration::new(self.ctime_sec, self.ctime_nsec),
            crtime: std::time::UNIX_EPOCH + std::time::Duration::new(self.crtime_sec, self.crtime_nsec),
            kind: match self.kind {
                1 => FileType::NamedPipe,
                2 => FileType::CharDevice,
                3 => FileType::BlockDevice,
                4 => FileType::Directory,
                5 => FileType::RegularFile,
                6 => FileType::Symlink,
                7 => FileType::Socket,
                _ => FileType::RegularFile, // Default fallback
            },
            perm: self.perm,
            nlink: self.nlink,
            uid: self.uid,
            gid: self.gid,
            rdev: self.rdev,
            flags: self.flags,
            blksize: self.blksize,
        }
    }
}

// QR-related data structures
#[derive(Serialize, Deserialize, Debug)]
pub struct FilesystemMetadata {
    pub version: u32,
    pub files: Vec<FileEntry>,
    pub next_inode: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase_hash: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct FileEntry {
    pub inode: u64,
    pub name: String,
    pub qr_blocks: Vec<u32>,
    pub parent: u64,
    pub attrs: SerializableFileAttr,
}

fn new_attrs(file_inode: u64, size: u64, is_folder: bool) -> FileAttr {
    let now = SystemTime::now();
    let duration_since_epoch = now.duration_since(std::time::UNIX_EPOCH).unwrap();
    
    FileAttr {
        ino: file_inode,
        size,
        blocks: 0,
        atime: now,
        mtime: now,
        ctime: now,
        crtime: now,
        kind: if is_folder {
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
    }
}

struct FSEntry {
    pub inode: u64,
    pub name: [u8; 25],
    pub data: Option<Vec<u8>>,
    pub parent: u64,
    pub children: Vec<u64>,
    pub attrs: FileAttr,
}

fn fixed_name(name: &str) -> [u8; 25] {
    let mut buf = [0u8; 25];
    let bytes = name.as_bytes();
    let len = bytes.len().min(25);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf
}

fn fixed_name_to_str(buf: &[u8; 25]) -> &str {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(25);
    std::str::from_utf8(&buf[..end]).unwrap_or("")
}

impl FSEntry {
    pub fn new(file_inode: u64, file_name: String, file_data: Option<Vec<u8>>, parent_inode: u64, file_attrs: &FileAttr) -> Self {
        Self {
            inode: file_inode,
            name: fixed_name(&file_name),
            data: file_data,
            parent: parent_inode,
            children: Vec::new(),
            attrs: *file_attrs,
        }
    }
}

struct QRFileSystem {
    pub files: HashMap<u64, FSEntry>,
}

impl QRFileSystem {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    pub fn push(&mut self, inode: u64, file_name: String, data: Option<Vec<u8>>, parent_inode: u64, file_attrs: &FileAttr) {
        let file: FSEntry = FSEntry::new(inode, file_name, data, parent_inode, file_attrs);
        self.files.insert(inode, file);

        if parent_inode != 0 {
            if let Some(parent) = self.files.get_mut(&parent_inode) {
                parent.children.push(inode);
            }
        }
    }

    pub fn rename(&mut self, old_parent_inode: u64, file_old_name: String, new_parent_inode: u64, file_new_name: String) {
        let mut found_child_inode: Option<u64> = None;

        if let Some(parent_file) = self.files.get(&old_parent_inode) {
            for &child_inode in &parent_file.children {
                if let Some(child) = self.files.get(&child_inode) {
                    if fixed_name_to_str(&child.name) == file_old_name {
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
            child.name = fixed_name(&file_new_name);
            child.parent = new_parent_inode;
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
            next_inode: INODE_COUNTER.load(Ordering::Relaxed),
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
                inode: file.inode,
                name: fixed_name_to_str(&file.name).to_string(),
                qr_blocks: vec![0; chunk_count],
                parent: file.parent,
                attrs: SerializableFileAttr::from_file_attr(&file.attrs),
            };
            metadata.files.push(entry);
            println!("  - {} (inode: {}, {} chunks)", fixed_name_to_str(&file.name), file.inode, chunk_count);
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
            if let Some(file) = self.files.get(&file_entry.inode) {
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
        println!("Next inode counter will be: {}", metadata.next_inode);
        
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
        
        println!("Found {} entries in directory", metadata.files.len());
        println!("Restoring inode counter to: {}", metadata.next_inode);
        
        self.files.clear();
        INODE_COUNTER.store(metadata.next_inode, Ordering::Relaxed);
        
        let mut sorted_files: Vec<&FileEntry> = metadata.files.iter().collect();
        sorted_files.sort_by_key(|f| f.inode);
        
        for file_entry in sorted_files {
            let mut file_data = Vec::new();
            
            if !file_entry.qr_blocks.is_empty() {
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
            }
            
            let file_attrs = file_entry.attrs.to_file_attr();
            
            self.push(
                file_entry.inode,
                file_entry.name.clone(),
                if file_data.is_empty() { None } else { Some(file_data) },
                file_entry.parent,
                &file_attrs
            );
            
            let file_type = if file_attrs.kind == FileType::Directory { "directory" } else { "file" };
            println!("Imported {}: '{}' (inode: {}, parent: {})", 
                    file_type, file_entry.name, file_entry.inode, file_entry.parent);
        }
        
        println!("\n=== Import completed successfully ===");
        println!("Total entries: {}", self.files.len());
        println!("Inode counter restored to: {}", metadata.next_inode);
        
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
        let file: &mut FSEntry = match self.files.get_mut(&ino) {
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
        let inode = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.push(inode, file_name, None, parent, &new_attrs(inode, 0, false));
        
        match self.files.get(&inode) {
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
        let inode = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.push(inode, file_name.to_string(), None, parent, &new_attrs(inode, 0, true));
        let file = self.files.get(&inode).unwrap();

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
                if fixed_name_to_str(&child.name) == name {
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

        let name_str = name.to_str().unwrap();

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
                if fixed_name_to_str(&child.name) == name_str {
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

            let parent = if dir.parent != 0 {
                dir.parent
            } else {
                ino
            };
            let _ = reply.add(parent, 2, FileType::Directory, "..");
        }

        let mut index = offset - 2;
        if index < 0 { index = 0; }

        let children = &dir.children;

        for (i, &child_inode) in children.iter().enumerate().skip(index as usize) { 
            if let Some(child) = self.files.get(&child_inode) {
                let next_offset = 3 + i as i64; 
                let name = fixed_name_to_str(&child.name);
                let _ = reply.add(child.inode, next_offset, child.attrs.kind, name);
            }
        }

        reply.ok();
    }

}


fn main() {
    use std::env;
    use std::io::{self, Write};
    
    println!("=== QR Filesystem Test ===");
    
    let args: Vec<String> = env::args().collect();
    
    if args.len() > 1 {
        match args[1].as_str() {
            "--export" => {
                let output_dir = if args.len() > 2 { &args[2] } else { "./qr_export" };
                let passphrase = if args.len() > 3 { &args[3] } else { "test123" };
                
                println!("Creating test filesystem and exporting to QR codes...");
                
                let mut fs = QRFileSystem::new();
                
                fs.push(1, "/".to_string(), None, 0, &new_attrs(1, 0, true));
                
                fs.push(2, "documents".to_string(), None, 1, &new_attrs(2, 0, true));
                fs.push(3, "images".to_string(), None, 1, &new_attrs(3, 0, true));
                fs.push(4, "projects".to_string(), None, 1, &new_attrs(4, 0, true));
                
                fs.push(5, "readme.txt".to_string(), 
                        Some(b"Welcome to QR Filesystem!\nThis is a test file.".to_vec()), 
                        1, &new_attrs(5, 45, false));
                
                fs.push(6, "notes.txt".to_string(), 
                        Some(b"Important notes:\n- Item 1\n- Item 2\n- Item 3".to_vec()), 
                        2, &new_attrs(6, 50, false));
                
                fs.push(7, "config.json".to_string(), 
                        Some(br#"{"version": "1.0", "author": "QRFS"}"#.to_vec()), 
                        2, &new_attrs(7, 40, false));
                
                fs.push(8, "data.bin".to_string(), 
                        Some(vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]), 
                        3, &new_attrs(8, 8, false));
                
                INODE_COUNTER.store(9, Ordering::Relaxed);
                
                println!("Test filesystem created with {} entries:", fs.files.len());
                for (inode, file) in &fs.files {
                    let file_type = if file.attrs.kind == FileType::Directory { "dir" } else { "file" };
                    println!("  [{}] {}: '{}' (parent: {})", 
                            inode, file_type, fixed_name_to_str(&file.name), file.parent);
                }
                
                println!("\nExporting to: {}", output_dir);
                if let Err(e) = fs.export_files_as_qr(output_dir, passphrase) {
                    eprintln!("Export failed: {}", e);
                    return;
                }
                
                println!("\n=== Export completed successfully! ===");
                println!("QR codes saved to: {}", output_dir);
                println!("Passphrase: '{}'", passphrase);
                println!("You can now import this filesystem using: --import");
                return;
            }
            
            "--import" => {
                let input_dir = if args.len() > 2 { &args[2] } else { "./qr_export" };
                let passphrase = if args.len() > 3 { &args[3] } else { "test123" };
                let mountpoint = if args.len() > 4 { &args[4] } else { 
                    println!("No mountpoint specified for import.");
                    println!("Usage: {} --import [input_dir] [passphrase] <MOUNTPOINT>", args[0]);
                    return;
                };
                
                println!("Importing filesystem from QR codes...");
                
                let mut fs = QRFileSystem::new();
                if let Err(e) = fs.import_files_from_qr(input_dir, passphrase) {
                    eprintln!("Import failed: {}", e);
                    return;
                }
                
                println!("\nImported filesystem structure:");
                for (inode, file) in &fs.files {
                    let file_type = if file.attrs.kind == FileType::Directory { "dir" } else { "file" };
                    let size_info = if file.attrs.kind == FileType::Directory { 
                        format!("({} children)", file.children.len())
                    } else {
                        format!("({} bytes)", file.attrs.size)
                    };
                    println!("  [{}] {}: '{}' {} (parent: {})", 
                            inode, file_type, fixed_name_to_str(&file.name), size_info, file.parent);
                }
                
                println!("\nMounting at: {}", mountpoint);
                match fuser::mount2(fs, mountpoint, &[]) {
                    Ok(_) => {
                        println!("Mounted successfully!");
                        println!("Try these commands:");
                        println!("  ls -la {}", mountpoint);
                        println!("  ls {}/documents", mountpoint);
                        println!("  cat {}/readme.txt", mountpoint);
                        println!("  cat {}/documents/notes.txt", mountpoint);
                        println!("\nUse 'fusermount -u {}' to unmount", mountpoint);
                    },
                    Err(e) => {
                        eprintln!("Mount failed: {:?}", e);
                        println!("Note: You may need to create the mountpoint directory first");
                    }
                }
                return;
            }
            
            "--test-roundtrip" => {
                println!("Testing complete QR roundtrip...");
                
                let mut original_fs = QRFileSystem::new();
                original_fs.push(1, "/".to_string(), None, 0, &new_attrs(1, 0, true));
                original_fs.push(2, "test_dir".to_string(), None, 1, &new_attrs(2, 0, true));
                original_fs.push(3, "test_file.txt".to_string(), 
                                Some(b"Hello QR World!".to_vec()), 
                                1, &new_attrs(3, 15, false));
                INODE_COUNTER.store(4, Ordering::Relaxed);
                
                let temp_dir = "./test_roundtrip";
                let passphrase = "test_passphrase";
                
                println!("1. Exporting filesystem...");
                if let Err(e) = original_fs.export_files_as_qr(temp_dir, passphrase) {
                    eprintln!("Export failed: {}", e);
                    return;
                }
                
                println!("2. Importing filesystem...");
                let mut imported_fs = QRFileSystem::new();
                if let Err(e) = imported_fs.import_files_from_qr(temp_dir, passphrase) {
                    eprintln!("Import failed: {}", e);
                    return;
                }
                
                println!("3. Verifying imported data...");
                let mut success = true;
                
                for (inode, original_file) in &original_fs.files {
                    if let Some(imported_file) = imported_fs.files.get(inode) {
                        if fixed_name_to_str(&original_file.name) != fixed_name_to_str(&imported_file.name) {
                            eprintln!("Name mismatch for inode {}: '{}' vs '{}'", 
                                    inode, fixed_name_to_str(&original_file.name), fixed_name_to_str(&imported_file.name));
                            success = false;
                        }
                        
                        if original_file.parent != imported_file.parent {
                            eprintln!("Parent mismatch for inode {}: {} vs {}", 
                                    inode, original_file.parent, imported_file.parent);
                            success = false;
                        }
                        
                        if original_file.attrs.kind != imported_file.attrs.kind {
                            eprintln!("Type mismatch for inode {}: {:?} vs {:?}", 
                                    inode, original_file.attrs.kind, imported_file.attrs.kind);
                            success = false;
                        }
                        
                        if let (Some(orig_data), Some(imp_data)) = (&original_file.data, &imported_file.data) {
                            if orig_data != imp_data {
                                eprintln!("Data mismatch for inode {}: {:?} vs {:?}", 
                                        inode, orig_data, imp_data);
                                success = false;
                            }
                        }
                    } else {
                        eprintln!("Missing file in imported FS: inode {}", inode);
                        success = false;
                    }
                }
                
                let _ = std::fs::remove_dir_all(temp_dir);
                
                if success {
                    println!("=== Roundtrip test PASSED! ===");
                } else {
                    eprintln!("=== Roundtrip test FAILED! ===");
                }
                return;
            }
            
            _ => {
                let mountpoint = &args[1];
                
                println!("Creating simple filesystem and mounting...");
                
                let mut fs = QRFileSystem::new();
                
                fs.push(1, "/".to_string(), None, 0, &new_attrs(1, 0, true));
                fs.push(2, "hello.txt".to_string(), 
                        Some(b"Hello from QR Filesystem!".to_vec()), 
                        1, &new_attrs(2, 25, false));
                fs.push(3, "empty_dir".to_string(), None, 1, &new_attrs(3, 0, true));
                
                INODE_COUNTER.store(4, Ordering::Relaxed);
                
                println!("Mounting at: {}", mountpoint);
                match fuser::mount2(fs, mountpoint, &[]) {
                    Ok(_) => {
                        println!("Mounted successfully!");
                        println!("Try: cat {}/hello.txt", mountpoint);
                        println!("Use 'fusermount -u {}' to unmount", mountpoint);
                    },
                    Err(e) => println!("Mount failed: {:?}", e),
                }
                return;
            }
        }
    }
    
    println!("Usage:");
    println!("  {} <MOUNTPOINT>                    - Mount simple test filesystem", args[0]);
    println!("  {} --export [dir] [passphrase]     - Create and export test filesystem", args[0]);
    println!("  {} --import [dir] [passphrase] <MOUNTPOINT> - Import and mount filesystem", args[0]);
    println!("  {} --test-roundtrip                - Test complete export/import cycle", args[0]);
    println!("\nExamples:");
    println!("  {} /mnt/qrfs                       # Mount simple FS", args[0]);
    println!("  {} --export ./my_qrfs mypass       # Export to QR codes", args[0]);
    println!("  {} --import ./my_qrfs mypass /mnt  # Import and mount", args[0]);
}