use std::collections::HashMap;
use std::env;
use std::time::Duration;
use std::ffi::OsStr;
use fuser::{ FileAttr, FileType, Filesystem, Request, ReplyDirectory, ReplyAttr, ReplyData, ReplyEntry, ReplyEmpty, ReplyOpen, ReplyCreate, ReplyWrite, TimeOrNow };
use libc::{ENOENT};
use std::time::{SystemTime, UNIX_EPOCH, Duration as StdDuration};
use serde::{Serialize, Deserialize};



#[derive(Debug, Clone)]
struct File {
    pub inode: u64,
    pub name: String,
    pub data: Option<Vec<u8>>,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub attrs: FileAttr,
}

impl File {
    pub fn new(inode: u64, file_name: String, file_data: Option<Vec<u8>>, parent_inode: Option<u64>, folder_flag: bool) -> Self {
        let size = match &file_data {
            Some(v) => v.len() as u64,
            None => 0,
        };

        let attr = FileAttr {
            ino: inode,
            size,
            blocks: 0,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: if folder_flag { FileType::Directory } else { FileType::RegularFile },
            perm: if folder_flag { 0o755 } else { 0o644 },
            nlink: 0,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 4096,
        };

        Self {
            inode,
            name: file_name,
            data: file_data,
            parent: parent_inode,
            children: Vec::new(),
            attrs: attr,
        }
    }

    pub fn new_root() -> Self {
        File::new(1, "/".into(), None, None, true)
    }

    pub fn to_disk(&self) -> FileDisk {
        fn tp(st: SystemTime) -> (i64, u32) {
            match st.duration_since(UNIX_EPOCH) {
                Ok(d) => (d.as_secs() as i64, d.subsec_nanos()),
                Err(e) => { let dur = e.duration(); (-(dur.as_secs() as i64), dur.subsec_nanos()) }
            }
        }

        let (at_s, at_ns) = tp(self.attrs.atime);
        let (mt_s, mt_ns) = tp(self.attrs.mtime);
        let (ct_s, ct_ns) = tp(self.attrs.ctime);
        let (crt_s, crt_ns) = tp(self.attrs.crtime);

        FileDisk {
            inode: self.inode,
            name: self.name.clone(),
            parent: self.parent,
            children: self.children.clone(),
            is_dir: self.attrs.kind == FileType::Directory,
            data: self.data.clone().unwrap_or_default(),

            size: self.attrs.size,
            blocks: self.attrs.blocks,
            atime_secs: at_s,
            atime_nanos: at_ns,
            mtime_secs: mt_s,
            mtime_nanos: mt_ns,
            ctime_secs: ct_s,
            ctime_nanos: ct_ns,
            crtime_secs: crt_s,
            crtime_nanos: crt_ns,

            perm: self.attrs.perm,
            nlink: self.attrs.nlink,
            uid: self.attrs.uid,
            gid: self.attrs.gid,
            rdev: self.attrs.rdev,
            flags: self.attrs.flags,
            blksize: self.attrs.blksize,
        }
    }

    pub fn from_disk(d: FileDisk) -> Self {
        fn from_tp(secs: i64, nanos: u32) -> SystemTime {
            if secs >= 0 {
                UNIX_EPOCH + StdDuration::new(secs as u64, nanos)
            } else {
                UNIX_EPOCH - StdDuration::new((-secs) as u64, nanos)
            }
        }

        let attrs = FileAttr {
            ino: d.inode,
            size: d.size,
            blocks: d.blocks,
            atime: from_tp(d.atime_secs, d.atime_nanos),
            mtime: from_tp(d.mtime_secs, d.mtime_nanos),
            ctime: from_tp(d.ctime_secs, d.ctime_nanos),
            crtime: from_tp(d.crtime_secs, d.crtime_nanos),
            kind: if d.is_dir { FileType::Directory } else { FileType::RegularFile },
            perm: d.perm,
            nlink: d.nlink,
            uid: d.uid,
            gid: d.gid,
            rdev: d.rdev,
            flags: d.flags,
            blksize: d.blksize,
        };

        File {
            inode: d.inode,
            name: d.name,
            data: if d.data.is_empty() { None } else { Some(d.data) },
            parent: d.parent,
            children: d.children,
            attrs,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct FileDisk {
    pub inode: u64,
    pub name: String,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub is_dir: bool,
    pub data: Vec<u8>,
    pub size: u64,
    pub blocks: u64,
    pub atime_secs: i64,
    pub atime_nanos: u32,
    pub mtime_secs: i64,
    pub mtime_nanos: u32,
    pub ctime_secs: i64,
    pub ctime_nanos: u32,
    pub crtime_secs: i64,
    pub crtime_nanos: u32,

    pub perm: u16,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u32,
    pub flags: u32,
    pub blksize: u32,
}

#[derive(Serialize, Deserialize, Debug)]
struct DiskFS {
    pub files: Vec<FileDisk>,
    pub next_inode: u64,
}

struct QRFileSystem {
    pub files: HashMap<u64, File>,
    pub next_inode: u64,
    pub persist_path: String,
}

impl QRFileSystem {

    pub fn save_to_disk(&self) -> Result<(), Box<dyn std::error::Error>> {
        let disk_vec: Vec<FileDisk> = self.files.values().map(|f| f.to_disk()).collect();
        let diskfs = DiskFS { files: disk_vec, next_inode: self.next_inode };
        let encoded: Vec<u8> = bincode::serialize(&diskfs)?;

        let tmp = format!("{}.tmp", &self.persist_path);
        std::fs::write(&tmp, encoded)?;
        std::fs::rename(&tmp, &self.persist_path)?;
        Ok(())
    }

    pub fn new_or_load(path: &str) -> QRFileSystem {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(diskfs) = bincode::deserialize::<DiskFS>(&bytes) {
                let mut files = HashMap::new();
                for d in diskfs.files.into_iter() {
                    let f = File::from_disk(d);
                    files.insert(f.inode, f);
                }
                let mut next = diskfs.next_inode;
                let max_inode = files.keys().copied().max().unwrap_or(0);
                if next <= max_inode { next = max_inode + 1; }
                return QRFileSystem { files, next_inode: next, persist_path: path.to_string() };
            } else {
                eprintln!("Advertencia: no se pudo deserializar '{}', creando FS nuevo.", path);
            }
        }

        let mut map = HashMap::new();
        let root = File::new_root();
        map.insert(1, root);
        QRFileSystem { files: map, next_inode: 2, persist_path: path.to_string() }
    }

    pub fn push(&mut self, file_name: String, data: Option<Vec<u8>>, parent_inode: Option<u64>, folder_flag: bool) -> u64 {
        let file: File = File::new(self.next_inode, file_name, data, parent_inode, folder_flag);
        let inode = file.inode;
        self.files.insert(inode, file);

        if let Some(pid) = parent_inode {
            if let Some(parent) = self.files.get_mut(&pid) {
                parent.children.push(inode);
            }
        }

        self.next_inode += 1;
        if let Err(e) = self.save_to_disk() {
            eprintln!("Warning: failed to save FS after push(): {:?}", e);
        }

        inode
    }

    // rename (mueve/renombra) y guarda
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

        let child_inode = match found_child_inode { Some(i) => i, None => return };

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

        if let Err(e) = self.save_to_disk() {
            eprintln!("Warning: failed to save FS after rename(): {:?}", e);
        }
    }
}

impl Filesystem for QRFileSystem {

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        match self.files.get(&ino) {
            Some(file) => {
                let ttl = Duration::from_secs(1);
                reply.attr(&ttl, &file.attrs);
            },
            None => reply.error(ENOENT),
        }
    }

    fn rename(&mut self, _req: &Request, parent: u64, name: &OsStr, newparent: u64, newname: &OsStr, _flags: u32, reply: ReplyEmpty) {
        let old_name = match name.to_str() { Some(s) => s.to_string(), None => { reply.error(ENOENT); return; } };
        let new_name = match newname.to_str() { Some(s) => s.to_string(), None => { reply.error(ENOENT); return; } };
        // llama a la funcion que guarda
        self.rename(parent, old_name, newparent, new_name);
        reply.ok();
    }

    fn write(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, data: &[u8], _write_flags: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyWrite) {
        let file: &mut File = match self.files.get_mut(&ino) {
            Some(f) => f,
            None => { reply.error(ENOENT); return; }
        };

        if file.attrs.kind == FileType::Directory {
            reply.error(ENOENT);
            return;
        }

        if file.data.is_none() { file.data = Some(Vec::new()); }

        let buffer = file.data.as_mut().unwrap();

        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }
        let offset = offset as usize;
        let required_size = offset.checked_add(data.len()).unwrap_or(usize::MAX);

        if buffer.len() < required_size {
            buffer.resize(required_size, 0);
        }

        buffer[offset..offset + data.len()].copy_from_slice(data);
        file.attrs.size = buffer.len() as u64;

        // Guardar después de escribir
        if let Err(e) = self.save_to_disk() {
            eprintln!("Warning: failed to save FS after write(): {:?}", e);
        }

        reply.written(data.len() as u32);
    }

    fn access(&mut self, _req: &Request, ino: u64, mask: i32, reply: ReplyEmpty) {
        let file = match self.files.get(&ino) {
            Some(f) => f,
            None => { reply.error(ENOENT); return; }
        };

        let perm = file.attrs.perm as i32;

        const R_OK: i32 = 4;
        const W_OK: i32 = 2;
        const X_OK: i32 = 1;

        if (mask & R_OK) != 0 && (perm & 0o444 == 0) {
            reply.error(libc::EACCES); return;
        }
        if (mask & W_OK) != 0 && (perm & 0o222 == 0) {
            reply.error(libc::EACCES); return;
        }
        if (mask & X_OK) != 0 && (perm & 0o111 == 0) {
            reply.error(libc::EACCES); return;
        }
        reply.ok();
    }

    fn create(&mut self, _req: &Request, parent: u64, name: &OsStr, _mode: u32, _umask: u32, flags: i32, reply: ReplyCreate) {
        let file_name = match name.to_str() { Some(s) => s.to_string(), None => { reply.error(ENOENT); return; } };
        let ino = self.push(file_name, None, Some(parent), false);
        match self.files.get(&ino) {
            Some(file) => {
                let ttl = Duration::from_secs(1);
                reply.created(&ttl, &file.attrs, 0, 0, flags.try_into().unwrap());
            },
            None => reply.error(ENOENT),
        }
    }

    fn open(&mut self, _req: &Request, ino: u64, flags: i32, reply: ReplyOpen) {
        let file = match self.files.get(&ino) {
            Some(f) => f,
            None => { reply.error(ENOENT); return; }
        };

        let write_mode = flags & (libc::O_WRONLY | libc::O_RDWR) != 0;
        if file.attrs.kind == FileType::Directory && write_mode {
            reply.error(libc::EISDIR); return;
        }

        let fh = ino;
        reply.opened(fh, 0);
    }

    fn setattr(
        &mut self,
        req: &Request<'_>,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        ctime: Option<SystemTime>,
        fh: Option<u64>,
        crtime: Option<SystemTime>,
        chgtime: Option<SystemTime>,
        bkuptime: Option<SystemTime>,
        flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        let file = match self.files.get_mut(&ino) {
            Some(f) => f,
            None => {
                reply.error(ENOENT);
                return;
            }
        };
        reply.attr(&Duration::new(1, 0), &file.attrs);
        drop(file);
        if let Err(e) = self.save_to_disk() {
            eprintln!("Error saving FS: {}", e);
        }
    }


    fn mkdir(&mut self, _req: &Request, parent: u64, name: &OsStr, _mode: u32, _umask: u32, reply: ReplyEntry) {
        let file_name = match name.to_str() { Some(s) => s, None => { reply.error(ENOENT); return; } };
        let ino = self.push(file_name.to_string(), None, Some(parent), true);
        let file = self.files.get(&ino).unwrap();
        reply.entry(&Duration::new(1, 0), &file.attrs, 0);
    }

    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let name = match name.to_str() { Some(s) => s, None => { reply.error(ENOENT); return; } };

        let children = match self.files.get(&parent) {
            Some(f) => f.children.clone(),
            None => { reply.error(ENOENT); return; }
        };

        let mut target_inode: Option<u64> = None;

        for child_inode in children {
            if let Some(child) = self.files.get(&child_inode) {
                if child.name == name {
                    if child.attrs.kind != FileType::Directory { reply.error(libc::ENOTDIR); return; }
                    if !child.children.is_empty() { reply.error(libc::ENOTEMPTY); return; }
                    target_inode = Some(child_inode);
                    break;
                }
            }
        }

        let target_inode = match target_inode { Some(i) => i, None => { reply.error(ENOENT); return; } };

        if let Some(parent_file) = self.files.get_mut(&parent) {
            parent_file.children.retain(|&x| x != target_inode);
        }

        self.files.remove(&target_inode);

        if let Err(e) = self.save_to_disk() {
            eprintln!("Warning: failed to save FS after rmdir(): {:?}", e);
        }

        reply.ok();
    }

    fn read(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        let file = match self.files.get(&ino) {
            Some(f) => f,
            None => { reply.error(ENOENT); return; }
        };

        if file.attrs.kind == FileType::Directory { reply.error(ENOENT); return; }

        let data = match &file.data {
            Some(d) => d,
            None => { reply.data(&[]); return; }
        };

        if offset < 0 { reply.error(libc::EINVAL); return; }
        let start = offset as usize;
        if start >= data.len() { reply.data(&[]); return; }
        let end = std::cmp::min(start + size as usize, data.len());
        reply.data(&data[start..end]);
    }

    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = match name.to_str() { Some(s) => s.to_string(), None => { reply.error(ENOENT); return; } };

        let parent_file = match self.files.get(&parent) {
            Some(p) => p,
            None => { reply.error(ENOENT); return; }
        };

        let mut found_inode: Option<u64> = None;

        for &child_inode in &parent_file.children {
            if let Some(child) = self.files.get(&child_inode) {
                if child.name == name_str { found_inode = Some(child_inode); break; }
            }
        }

        let inode = match found_inode { Some(i) => i, None => { reply.error(ENOENT); return; } };
        let file = self.files.get(&inode).unwrap();
        reply.entry(&Duration::from_secs(1), &file.attrs, 0);
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        let dir = match self.files.get(&ino) {
            Some(f) => f,
            None => { reply.error(ENOENT); return; }
        };

        if dir.attrs.kind != FileType::Directory { reply.error(ENOENT); return; }

        if offset == 0 {
            let _ = reply.add(ino, 1, FileType::Directory, ".");
            let parent = dir.parent.unwrap_or(ino);
            let _ = reply.add(parent, 2, FileType::Directory, "..");
        }

        let start_index: usize = if offset <= 2 { 0 } else { (offset - 2) as usize };

        let children = &dir.children;

        for (i, &child_inode) in children.iter().enumerate().skip(start_index) {
            if let Some(child) = self.files.get(&child_inode) {
                let next_offset = 3 + i as i64;
                let _ = reply.add(child.inode, next_offset, child.attrs.kind, child.name.as_str());
            }
        }

        reply.ok();
    }
}

fn main() {
    let store_path = "fs.bin";
    let mut fs = QRFileSystem::new_or_load(store_path);

    // if fs.files.len() == 1 {
    //     fs.push("pingapeta".to_string(), None, Some(1), true);
    //     fs.push("fileB.txt".to_string(), Some(b"Contenido B".to_vec()), Some(1), false);
    //     fs.push("fileC.txt".to_string(), Some(b"Contenido C".to_vec()), Some(1), false);
    // }

    let mountpoint = match env::args().nth(1) {
        Some(p) => p,
        None => { println!("Usage: <program> <MOUNTPOINT>"); return; }
    };

    match fuser::mount2(fs, &mountpoint, &[]) {
        Ok(_) => println!("Mounted successfully"),
        Err(e) => println!("ERROR MOUNTING: {:?}", e),
    }
}
