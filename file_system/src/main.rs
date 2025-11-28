use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::env;
use std::ffi::OsStr;
use fuser::{ FileAttr, FileType, Filesystem, Request, ReplyDirectory, ReplyAttr, ReplyData, ReplyEntry, ReplyEmpty, ReplyOpen, ReplyCreate, ReplyWrite };
use libc::{ENOENT};


use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH, Duration};


const BLOCK_COUNT: u64 = 2048; 
const BLOCK_SIZE: u64 = 512;
const MAX_NAME_SIZE: usize = 25;
const BITMAP_START: u64 = 0;
const INODE_COUNTER_START: u64 = 1;
const DATA_START: u64 = 2;


fn to_seconds(t: SystemTime) -> u64 {
    let seconds_since_epoch = t
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    seconds_since_epoch
}


fn u64_to_systemtime(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}


fn serialize_fs_entry_to_disk(f: &FSEntry) -> Vec<u8> {
    let mut buf = Vec::new();
    
    buf.extend_from_slice(&f.inode.to_le_bytes());
    buf.extend_from_slice(&f.parent.to_le_bytes());
    buf.extend_from_slice(&f.name);

    let is_directory: u8 = if f.attrs.kind == FileType::Directory { 1 } else { 0 };
    buf.push(is_directory);

    buf.extend_from_slice(&f.attrs.perm.to_le_bytes());
    buf.extend_from_slice(&to_seconds(f.attrs.atime.into()).to_le_bytes());
    buf.extend_from_slice(&to_seconds(f.attrs.mtime.into()).to_le_bytes());
    buf.extend_from_slice(&to_seconds(f.attrs.ctime.into()).to_le_bytes());
    buf.extend_from_slice(&to_seconds(f.attrs.crtime.into()).to_le_bytes());
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
    offset+=8;
    let file_parent = u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap());
    offset+=8;

    let mut file_name = [0u8; MAX_NAME_SIZE];
    file_name.copy_from_slice(&buf[offset..offset+MAX_NAME_SIZE]);
    offset+=MAX_NAME_SIZE;
    
    let is_directory = buf[offset];
    offset+=1;

    let file_perm = u16::from_le_bytes(buf[offset..offset+2].try_into().unwrap());
    offset+=2;
    let file_atime = u64_to_systemtime(u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap()));
    offset+=8;
    let file_mtime = u64_to_systemtime(u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap()));
    offset+=8;
    let file_ctime = u64_to_systemtime(u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap()));
    offset+=8;
    let file_crtime = u64_to_systemtime(u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap()));
    offset+=8;
    let file_blksize = u32::from_le_bytes(buf[offset..offset+4].try_into().unwrap());
    offset+=4;
    let file_data_size = u64::from_le_bytes(buf[offset..offset+8].try_into().unwrap());
    offset+=8;

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
        } else { FileType::RegularFile },
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

    let file = FSEntry { // just for tests
        inode: file_inode,
        name: file_name,
        data: if file_data_vec.is_empty() { None } else { Some(file_data_vec) },
        parent: file_parent,
        children: Vec::new(),
        attrs: attr,
    };

    file
}


fn open_disk(path: &str) -> std::io::Result<File> {
    OpenOptions::new().read(true).write(true).create(true).open(path)
}


fn write_u64(file: &mut File, offset: u64, v: u64) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(&v.to_le_bytes())?;
    Ok(())
}

fn read_u64(file: &mut File, offset: u64) -> std::io::Result<u64> {
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


fn initialize_new_disk(path: &str) -> std::io::Result<()> {
    let mut f = open_disk(path)?;

    let total_size = BLOCK_COUNT * BLOCK_SIZE;
    f.set_len(total_size)?;

    write_u64(&mut f, INODE_COUNTER_START * BLOCK_SIZE, 0)?; 

    let mut bitmap = vec![0u8; BLOCK_SIZE as usize];
    bitmap_set_bit(&mut bitmap, 0); 
    bitmap_set_bit(&mut bitmap, 1); 
    write_bitmap(&mut f, &bitmap)?;

    f.sync_all()?;
    println!("Disco inicializado: '{}' ({} bytes)", path, total_size);
    Ok(())
}

static INODE_COUNTER: AtomicU64 = AtomicU64::new(1);


fn get_default_attrs(file_inode: u64, size: u64, is_folder: bool) -> FileAttr {
    let attrs = FileAttr {
        ino: file_inode,
        size,
        blocks: 0,
        atime: std::time::SystemTime::now(),
        mtime: std::time::SystemTime::now(),
        ctime: std::time::SystemTime::now(),
        crtime: std::time::SystemTime::now(),
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
    };
    attrs
}


struct FSEntry {
    pub inode: u64,
    pub name:[u8; 25],
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
    pub inode_block_table: HashMap<u64, u64>,
    pub disk: File,
    pub bitmap: Vec<u8>,
}

impl QRFileSystem { //The root inode is always equals one
    pub fn new(path: &str) -> Self {
        let mut disk_file = open_disk(path).unwrap();
        let bm = read_bitmap(&mut disk_file).unwrap();
        Self {
            files: HashMap::new(),
            inode_block_table: HashMap::new(),
            disk: disk_file,
            bitmap: bm,
        }
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

    pub fn rename(&mut self, old_parent_inode: u64, file_old_name: String, new_parent_inode: u64, file_new_name: String,) {

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

        let _ = self.push(inode, file_name, None, parent, &get_default_attrs(inode, 0, false));

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
        let _ = self.push(inode, file_name.to_string(), None, parent, &get_default_attrs(inode, 0, true));
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

}


fn main() -> std::io::Result<()> {

    let path = "josh_disk.bin";
    let is_new_disk = !Path::new(path).exists();

    if is_new_disk {
        initialize_new_disk(path)?;
    }

    let mut fs = QRFileSystem::new(path);
    let _ = fs.load_fs_from_disk();

    let actual_inodes: u64 = read_u64(&mut fs.disk, INODE_COUNTER_START * BLOCK_SIZE)?; // may be a constant
    INODE_COUNTER.store(actual_inodes + 1, Ordering::Relaxed);

    // COMMENT THE INSERTIONS THE SECOND TIME YOU RUN THIS.

    let ino1 = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let ino2 = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let ino3 = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let ino4 = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);

    let _ = fs.push(ino1, "/".to_string(), None, 0, &get_default_attrs(ino1, 0, true));
    let _ = fs.push(ino2, "pingapeta".to_string(), None, 1, &get_default_attrs(ino2, 0, true));
    let len1 = b"Contenido B".to_vec().len() as u64;
    let len2 = b"Contenido C".to_vec().len() as u64;

    let _ = fs.push(ino3, "fileB.txt".to_string(), Some(b"Contenido B".to_vec()), 1, &get_default_attrs(ino3, len1, false));
    let _ = fs.push(ino4, "fileC.txt".to_string(), Some(b"Contenido C".to_vec()), 1, &get_default_attrs(ino4, len2, false));


    let mountpoint = match env::args().nth(1) {
        Some(p) => p,
        None => {
            println!("Usage: <program> <MOUNTPOINT>");
            return Ok(());
        }
    };

    match fuser::mount2(fs, &mountpoint, &[]) {
        Ok(_) => println!("Mounted successfully"),
        Err(e) => println!("ERROR MOUNTING: {:?}", e),
    }

    Ok(())
}

// Note: if you cloned the repository, you have to make a dir to mount the fs out of the repo
// run with cargo run -- ~/Desktop/fs
// To unmount, run the command: fusermount -u ~/Desktop/fs
// if you don't unmount, you'll run into errors next time you try cargo run.
// TO RUN THE PROGRAM YOU HAVE TO USE OTHER TERMINAL, DO NOT USE THE VS CODE TERMINAL.
