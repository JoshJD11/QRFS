pub use std::sync::atomic::Ordering;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64};
use std::time::Duration;
use std::ffi::OsStr;
use fuser::{FileAttr, Filesystem, Request, ReplyDirectory, ReplyAttr, ReplyData, ReplyEntry, ReplyEmpty, ReplyOpen, ReplyCreate, ReplyWrite, ReplyStatfs};
pub use fuser::FileType;
use libc::{ENOENT};
use std::time::SystemTime;
use qrcode::{QrCode, EcLevel};
use image::Luma;
use data_encoding::BASE64;
use std::fs;
use serde::{Serialize, Deserialize};
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub use std::time::{UNIX_EPOCH};

pub const BLOCK_COUNT: u64 = 2048; 
pub const BLOCK_SIZE: u64 = 512;
pub const MAX_NAME_SIZE: usize = 25;
pub const BITMAP_START: u64 = 0;
pub const INODE_COUNTER_START: u64 = 1;
pub const DATA_START: u64 = 2;

pub static INODE_COUNTER: AtomicU64 = AtomicU64::new(1);

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
                _ => FileType::RegularFile,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct FilesystemMetadata {
    pub version: u32,
    pub files: Vec<FileEntry>,
    pub next_inode: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase_hash: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileEntry {
    pub inode: u64,
    pub name: String,
    pub qr_blocks: Vec<u32>,
    pub parent: u64,
    pub attrs: SerializableFileAttr,
}

fn to_seconds(t: SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

fn u64_to_systemtime(secs: u64) -> SystemTime {
    std::time::UNIX_EPOCH + Duration::from_secs(secs)
}

fn serialize_fs_entry_to_disk(f: &FSEntry) -> Vec<u8> {
    let mut buf = Vec::new();
    
    buf.extend_from_slice(&f.inode.to_le_bytes());
    buf.extend_from_slice(&f.parent.to_le_bytes());
    buf.extend_from_slice(&f.name);

    let is_directory: u8 = if f.attrs.kind == FileType::Directory { 1 } else { 0 };
    buf.push(is_directory);

    buf.extend_from_slice(&f.attrs.perm.to_le_bytes());
    buf.extend_from_slice(&to_seconds(f.attrs.atime).to_le_bytes());
    buf.extend_from_slice(&to_seconds(f.attrs.mtime).to_le_bytes());
    buf.extend_from_slice(&to_seconds(f.attrs.ctime).to_le_bytes());
    buf.extend_from_slice(&to_seconds(f.attrs.crtime).to_le_bytes());
    buf.extend_from_slice(&f.attrs.blksize.to_le_bytes());
    buf.extend_from_slice(&f.attrs.size.to_le_bytes());
    
    if let Some(slice) = &f.data {
        let bytes: &[u8] = &slice;
        buf.extend_from_slice(bytes);
    }

    buf
}

fn deserialize_fs_entry(buf: &[u8]) -> FSEntry {
    let mut offset = 0;

    let file_inode = u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap());
    offset += 8;
    let file_parent = u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap());
    offset += 8;

    let mut file_name = [0u8; MAX_NAME_SIZE];
    file_name.copy_from_slice(&buf[offset..offset+MAX_NAME_SIZE]);
    offset += MAX_NAME_SIZE;
    
    let is_directory = buf[offset];
    offset += 1;

    let file_perm = u16::from_le_bytes(buf[offset..offset+2].try_into().unwrap());
    offset += 2;
    let file_atime = u64_to_systemtime(u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap()));
    offset += 8;
    let file_mtime = u64_to_systemtime(u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap()));
    offset += 8;
    let file_ctime = u64_to_systemtime(u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap()));
    offset += 8;
    let file_crtime = u64_to_systemtime(u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap()));
    offset += 8;
    let file_blksize = u32::from_le_bytes(buf[offset..offset+4].try_into().unwrap());
    offset += 4;
    let file_data_size = u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap());
    offset += 8;

    let attr = FileAttr {
        ino: file_inode,
        size: file_data_size,
        blocks: 0,
        atime: file_atime,
        mtime: file_mtime,
        ctime: file_ctime,
        crtime: file_crtime,
        kind: if is_directory != 0 {
            FileType::Directory
        } else { 
            FileType::RegularFile 
        },
        perm: file_perm,
        nlink: 0,
        uid: 0,
        gid: 0,
        rdev: 0,
        flags: 0,
        blksize: file_blksize,
    };

    let file_data = &buf[offset..offset+(file_data_size as usize)];
    let file_data_vec = file_data.to_vec();

    FSEntry {
        inode: file_inode,
        name: file_name,
        data: if file_data_vec.is_empty() { None } else { Some(file_data_vec) },
        parent: file_parent,
        children: Vec::new(),
        attrs: attr,
    }
}

fn open_disk(path: &str) -> std::io::Result<File> {
    OpenOptions::new().read(true).write(true).create(true).open(path)
}

pub fn write_u64(file: &mut File, offset: u64, v: u64) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(&v.to_le_bytes())?;
    Ok(())
}

pub fn read_u64(file: &mut File, offset: u64) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_bitmap(f: &mut File) -> std::io::Result<Vec<u8>> {
    let bitmap_bytes = BLOCK_SIZE as usize;
    let mut buf = vec![0u8; bitmap_bytes];
    let offset = BITMAP_START * BLOCK_SIZE;
    f.seek(SeekFrom::Start(offset))?;
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn write_bitmap(f: &mut File, bitmap: &[u8]) -> std::io::Result<()> {
    let offset = BITMAP_START * BLOCK_SIZE;
    f.seek(SeekFrom::Start(offset))?;
    f.write_all(bitmap)?;
    Ok(())
}

fn bitmap_get(bitmap: &[u8], idx: u64) -> bool {
    let byte_idx = (idx / 8) as usize;
    let bit = (idx % 8) as u8;
    if byte_idx >= bitmap.len() {
        return false;
    }
    (bitmap[byte_idx] & (1 << bit)) != 0
}

fn bitmap_set_bit(bitmap: &mut [u8], idx: u64) {
    let byte_idx = (idx / 8) as usize;
    let bit = (idx % 8) as u8;
    if byte_idx < bitmap.len() {
        bitmap[byte_idx] |= 1 << bit;
    }
}

fn bitmap_clear_bit(bitmap: &mut [u8], idx: u64) {
    let byte_idx = (idx / 8) as usize;
    let bit = (idx % 8) as u8;
    if byte_idx < bitmap.len() {
        bitmap[byte_idx] &= !(1 << bit);
    }
}

fn allocate_block(f: &mut File) -> std::io::Result<Option<u64>> {
    let mut bitmap = read_bitmap(f)?;
    for block in DATA_START..BLOCK_COUNT {
        if !bitmap_get(&bitmap, block) {
            bitmap_set_bit(&mut bitmap, block);
            write_bitmap(f, &bitmap)?;
            return Ok(Some(block));
        }
    }
    Ok(None)
}

fn free_block(f: &mut File, block_idx: u64) -> std::io::Result<()> {
    let mut bitmap = read_bitmap(f)?;
    bitmap_clear_bit(&mut bitmap, block_idx);
    write_bitmap(f, &bitmap)?;
    Ok(())
}

fn write_block(f: &mut File, block_idx: u64, data: &[u8]) -> std::io::Result<()> {
    if data.len() as u64 > BLOCK_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "data too large for block",
        ));
    }
    let offset = block_idx * BLOCK_SIZE;
    f.seek(SeekFrom::Start(offset))?;
    f.write_all(data)?;
    let pad = (BLOCK_SIZE as usize).saturating_sub(data.len());
    if pad > 0 {
        let zeros = vec![0u8; pad];
        f.write_all(&zeros)?;
    }
    Ok(())
}

fn read_block(f: &mut File, block_idx: u64) -> std::io::Result<Vec<u8>> {
    let offset = block_idx * BLOCK_SIZE;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; BLOCK_SIZE as usize];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn initialize_new_disk(path: &str) -> std::io::Result<()> {
    let mut f = open_disk(path)?;

    let total_size = BLOCK_COUNT * BLOCK_SIZE;
    f.set_len(total_size)?;

    write_u64(&mut f, INODE_COUNTER_START * BLOCK_SIZE, 0)?;

    let mut bitmap = vec![0u8; BLOCK_SIZE as usize];
    bitmap_set_bit(&mut bitmap, 0);
    bitmap_set_bit(&mut bitmap, 1);
    write_bitmap(&mut f, &bitmap)?;

    f.sync_all()?;
    // println!("Disk initialized: '{}' ({} bytes)", path, total_size);
    Ok(())
}

pub fn get_default_attrs(file_inode: u64, size: u64, is_folder: bool) -> FileAttr {
    let now = SystemTime::now();
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

pub struct FSEntry {
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

pub fn fixed_name_to_str(buf: &[u8; 25]) -> &str {
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

pub struct QRFileSystem {
    pub files: HashMap<u64, FSEntry>,
    pub inode_block_table: HashMap<u64, u64>,
    pub disk: File,
    pub bitmap: Vec<u8>,
    pub mount_path: String,
    pub auto_export_path: Option<String>,
    pub passphrase: Option<String>,
    pub modified: bool,
}

impl QRFileSystem {
    pub fn new(path: &str, mount_path: &str) -> Self {
        let mut disk_file = open_disk(path).unwrap();
        let bm = read_bitmap(&mut disk_file).unwrap();
        Self {
            files: HashMap::new(),
            inode_block_table: HashMap::new(),
            disk: disk_file,
            bitmap: bm,
            mount_path: mount_path.to_string(),
            auto_export_path: None,
            passphrase: None,
            modified: false,
        }
    }

    pub fn enable_auto_export(&mut self, export_path: &str, passphrase: &str) {
        self.auto_export_path = Some(export_path.to_string());
        self.passphrase = Some(passphrase.to_string());
        // println!("Auto-export enabled to: {}", export_path);
        // println!("Passphrase: {}", passphrase);
    }
    
    pub fn mark_modified(&mut self) {
        self.modified = true;
    }

    pub fn fill_children(&mut self) {
        let mut relations = Vec::new();

        for child in self.files.values() {
            let inode = child.inode;
            let parent_inode = child.parent;
            if parent_inode != 0 {
                relations.push((parent_inode, inode));
            }
        }

        for (parent_inode, inode) in relations {
            if let Some(parent) = self.files.get_mut(&parent_inode) {
                parent.children.push(inode);
            }
        }
    }

    pub fn load_fs_from_disk(&mut self) -> std::io::Result<()> {
        let bitmap = read_bitmap(&mut self.disk)?;
        for block in DATA_START..BLOCK_COUNT {
            if bitmap_get(&bitmap, block) {
                let data = read_block(&mut self.disk, block)?;
                let file: FSEntry = deserialize_fs_entry(&data);
                self.inode_block_table.insert(file.inode, block);
                self.files.insert(file.inode, file);
            }
        }
        self.fill_children();
        Ok(())
    }

    pub fn push(&mut self, inode: u64, file_name: String, data: Option<Vec<u8>>, parent_inode: u64, file_attrs: &FileAttr) -> std::io::Result<()> {
        let file: FSEntry = FSEntry::new(inode, file_name, data, parent_inode, file_attrs);
        
        self.files.insert(inode, file);

        let file_ref = self.files.get(&inode).unwrap();
        let serialized_data = serialize_fs_entry_to_disk(file_ref);

        let idx = allocate_block(&mut self.disk)?.expect("No free blocks available");
        self.inode_block_table.insert(inode, idx);
        
        write_block(&mut self.disk, idx, &serialized_data)?;
        bitmap_set_bit(&mut self.bitmap, idx);
        write_bitmap(&mut self.disk, &self.bitmap)?;

        if let Some(parent) = self.files.get_mut(&parent_inode) {
            parent.children.push(inode);
        }
        
        Ok(())
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
            let data_child = serialize_fs_entry_to_disk(child);
            let _ = write_block(&mut self.disk, *self.inode_block_table.get(&child.inode).unwrap(), &data_child);
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
        
        let code = QrCode::with_error_correction_level(
            base64_data.as_bytes(), 
            EcLevel::H
        )?;
        
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
        const MAX_QR_DATA_SIZE: usize = 512;
        
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

    fn clear_export_directory(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let path = std::path::Path::new(path);
        
        if !path.exists() {
            return Ok(());
        }
        
        
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let file_path = entry.path();
            
            if let Some(ext) = file_path.extension() {
                if ext == "png" {
                    std::fs::remove_file(&file_path)?;
                } else if file_path.is_file() && file_path.file_name().unwrap() != ".gitkeep" {
                    println!("  Warning: Non-QR file found: {}", file_path.display());
                }
            }
        }
        
        Ok(())
    }
    
    pub fn export_files_as_qr(&self, output_dir: &str, passphrase: &str) -> Result<(), Box<dyn std::error::Error>> {
        // if let Err(e) = self.clear_export_directory(output_dir) {
        //     return Err(format!("Failed to clear export directory '{}': {}", output_dir, e).into());
        // }
    
        fs::create_dir_all(output_dir)?;
        
        // println!("Exporting filesystem structure with passphrase protection...");
        
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
            // println!("  - {} (inode: {}, {} chunks)", fixed_name_to_str(&file.name), file.inode, chunk_count);
        }
        
        let mut current_block = 0;
        
        let metadata_json = serde_json::to_string(&metadata)?;
        // println!("Initial metadata size: {} bytes", metadata_json.len());
        
        let metadata_chunks = self.split_data_for_qr(metadata_json.as_bytes());
        // println!("Directory metadata requires {} QR blocks", metadata_chunks.len());
        
        for (chunk_index, chunk) in metadata_chunks.iter().enumerate() {
            let qr_path = format!("{}/{:03}.png", output_dir, current_block);
            self.binary_to_qr(chunk, &qr_path)?;
            // println!("  Created directory block {}: {}", chunk_index, qr_path);
            current_block += 1;
        }
        
        let directory_blocks_count = metadata_chunks.len() as u32;
        
        for file_entry in &mut metadata.files {
            if let Some(file) = self.files.get(&file_entry.inode) {
                if let Some(file_data) = &file.data {
                    let data_chunks = self.split_data_for_qr(&file_data);
                    
                    // println!("Exporting file '{}' as {} QR blocks...", file_entry.name, data_chunks.len());
                    
                    for (chunk_index, chunk) in data_chunks.iter().enumerate() {
                        let qr_path = format!("{}/{:03}.png", output_dir, current_block);
                        self.binary_to_qr(chunk, &qr_path)?;
                        file_entry.qr_blocks[chunk_index] = current_block;
                        // println!("  Created file block {}: {}", current_block, qr_path);
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
                let qr_path = format!("{}/{:03}.png", output_dir, chunk_index as u32);
                self.binary_to_qr(chunk, &qr_path)?;
                // println!("  Updated directory block {} with final metadata", chunk_index);
            } else {
                let qr_path = format!("{}/{:03}.png", output_dir, current_block);
                self.binary_to_qr(chunk, &qr_path)?;
                // println!("  Added directory block {}: {}", current_block, qr_path);
                current_block += 1;
            }
        }
        
        println!("Export completed! Total files: {}, Total QR blocks: {}", 
                metadata.files.len(), current_block);
        println!("Passphrase protection enabled. Remember your passphrase: '{}'", passphrase);
        // println!("Next inode counter will be: {}", metadata.next_inode);
        
        Ok(())
    }
    
    pub fn import_files_from_qr(&mut self, input_dir: &str, expected_passphrase: &str) -> Result<(), Box<dyn std::error::Error>> {
        // println!("Importing from QR codes in: {}", input_dir);
        // println!("Verifying passphrase...");
        
        let expected_hash = self.hash_passphrase(expected_passphrase);
        
        let mut directory_blocks = Vec::new();
        let mut current_block = 0;
        let mut found_passphrase = false;
        let mut final_metadata = None;
        
        loop {
            let qr_path = format!("{}/{:03}.png", input_dir, current_block);
            if !std::path::Path::new(&qr_path).exists() {
                break;
            }
            
            match self.qr_to_binary(&qr_path) {
                Ok(data) => {
                    directory_blocks.push(data);
                    current_block += 1;
                    // println!("  Read directory block {}", current_block - 1);
                    
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
        
        // println!("Found {} entries in directory", metadata.files.len());
        // println!("Restoring inode counter to: {}", metadata.next_inode);
        
        self.files.clear();
        self.inode_block_table.clear();
        INODE_COUNTER.store(metadata.next_inode, Ordering::Relaxed);
        
        let mut sorted_files: Vec<&FileEntry> = metadata.files.iter().collect();
        sorted_files.sort_by_key(|f| f.inode);
        
        for file_entry in sorted_files {
            let mut file_data = Vec::new();
            
            if !file_entry.qr_blocks.is_empty() {
                for &block_num in &file_entry.qr_blocks {
                    let qr_path = format!("{}/{:03}.png", input_dir, block_num);
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
            
            if let Err(e) = self.push(
                file_entry.inode,
                file_entry.name.clone(),
                if file_data.is_empty() { None } else { Some(file_data) },
                file_entry.parent,
                &file_attrs
            ) {
                return Err(format!("Failed to push file '{}': {}", file_entry.name, e).into());
            }
            
            let file_type = if file_attrs.kind == FileType::Directory { "directory" } else { "file" };
            println!("Imported {}: '{}' (inode: {}, parent: {})", 
                    file_type, file_entry.name, file_entry.inode, file_entry.parent);
        }
        
        println!("\n=== Import completed successfully ===");
        // println!("Total entries: {}", self.files.len());
        // println!("Inode counter restored to: {}", metadata.next_inode);
        
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
                return;
            }
        };

        if file.attrs.kind == FileType::Directory {
            reply.error(ENOENT);
            return;
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

        let serialized = serialize_fs_entry_to_disk(file);
        let block_idx = *self.inode_block_table.get(&ino).expect("missing block");
        let _ = write_block(&mut self.disk, block_idx, &serialized);

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
        let _ = write_u64(&mut self.disk, INODE_COUNTER_START * BLOCK_SIZE, inode);

        if let Err(e) = self.push(inode, file_name, None, parent, &get_default_attrs(inode, 0, false)) {
            reply.error(libc::EIO);
            return;
        }

        let file = match self.files.get(&inode) {
            Some(f) => f,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let attr = &file.attrs;
        let ttl = Duration::from_secs(1);

        let fh = inode;

        reply.created(&ttl, attr, 0, fh, flags.try_into().unwrap());
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

        let data = serialize_fs_entry_to_disk(file);
        let _ = write_block(&mut self.disk, *self.inode_block_table.get(&ino).unwrap(), &data);

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
        let _ = write_u64(&mut self.disk, INODE_COUNTER_START * BLOCK_SIZE, inode);
        if let Err(_) = self.push(inode, file_name.to_string(), None, parent, &get_default_attrs(inode, 0, true)) {
            reply.error(libc::EIO);
            return;
        }
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

        if let Some(&block_idx) = self.inode_block_table.get(&target_inode) {
            let _ = free_block(&mut self.disk, block_idx);
            self.inode_block_table.remove(&target_inode);
        }

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

    fn fsync(&mut self, _req: &Request, _ino: u64, _fh: u64, _datasync: bool, reply: ReplyEmpty) { // Innecesary in the actual implementation but the teacher ask for this method
        reply.ok();
    }

    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        let actual_cant_inodes = INODE_COUNTER.load(Ordering::Relaxed) - 1;
        let free_blocks = BLOCK_COUNT - actual_cant_inodes - 2;
        reply.statfs(
            BLOCK_COUNT,
            free_blocks,
            free_blocks,
            BLOCK_COUNT - 2,
            (BLOCK_COUNT - 2) - actual_cant_inodes,
            BLOCK_SIZE.try_into().unwrap(),
            MAX_NAME_SIZE.try_into().unwrap(),
            BLOCK_SIZE.try_into().unwrap(),
        );
    }

    fn destroy(&mut self) {
        // println!("FUSE destroy called - filesystem is unmounting");
        
        if self.auto_export_path.is_some() {
            let export_path = self.auto_export_path.as_ref().unwrap();
            let passphrase = self.passphrase.as_ref().unwrap();
            
            println!("Auto-exporting on unmount...");
            if let Err(e) = self.export_files_as_qr(export_path, passphrase) {
                eprintln!("Export failed: {}", e);
                // Try to save to a fallback location
                let fallback = format!("{}/emergency_backup_{}", 
                    std::env::temp_dir().display(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                );
                let _ = self.export_files_as_qr(&fallback, passphrase);
                eprintln!("Emergency backup saved to: {}", fallback);
            }
        }
    }


}
