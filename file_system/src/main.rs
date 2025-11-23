// extern crate rustc_serialize;

// use rustc_serialize::json::{self, Json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
// use std::path::Path;
use std::env;
use std::time::Duration;
use std::ffi::OsStr;
use fuser::{ FileAttr, FileType, Filesystem, Request, ReplyDirectory, ReplyAttr, ReplyData, ReplyEntry, ReplyEmpty, ReplyOpen, ReplyCreate, ReplyWrite };
use libc::{ENOENT};
use std::time::SystemTime;


static INODE_COUNTER: AtomicU64 = AtomicU64::new(1);

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


impl QRFileSystem { //The root node is always equals one
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

}


impl Filesystem for QRFileSystem {

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        // println!("getattr(ino={})", ino);
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


    // fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {

    // }


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

        // Cambiar permisos si se solicitaron
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


    // fn fsyncdir(&mut self, _req: &Request, ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {

    // }


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

        for (i, &child_inode) in children.iter().enumerate().skip(index as usize) { // It is neccesary to skip this time?
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
    fs.push("pingapeta".to_string(), None, Some(1), true);
    fs.push("fileB.txt".to_string(), Some(b"Contenido B".to_vec()), Some(1), false);
    fs.push("fileC.txt".to_string(), Some(b"Contenido C".to_vec()), Some(1), false);



    let mountpoint = match env::args().nth(1) {
        Some(p) => p,
        None => {
            println!("Usage: <program> <MOUNTPOINT>");
            return;
        }
    };

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
