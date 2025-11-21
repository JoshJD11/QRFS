// extern crate rustc_serialize;

// use rustc_serialize::json::{self, Json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
// use std::path::Path;
use std::env;
use std::time::Duration;
use std::ffi::OsStr;
use fuser::{ FileAttr, FileType, Filesystem, Request, ReplyDirectory, ReplyAttr, ReplyData, ReplyEntry, ReplyEmpty };
use libc::{ENOENT};
use qrcode::QrCode;
use image::Luma;
use data_encoding::BASE64;
use std::fs;


static INODE_COUNTER: AtomicU64 = AtomicU64::new(1);

struct File {
    pub inode: u64,
    pub name: String,
    pub data: Option<String>, // It's a String for now
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub attrs: FileAttr,
}

impl File {
    pub fn new(file_name: String, file_data: Option<String>, parent_inode: Option<u64>, folder_flag: bool) -> Self {

        let id: u64 = INODE_COUNTER.fetch_add(1, Ordering::Relaxed);

        let attr = FileAttr {
            ino: id,
            size: 0,
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

    pub fn push(&mut self, file_name: String, data: Option<String>, parent_inode: Option<u64>, folder_flag: bool) {
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
        
        // 200x200 pixel
        let image = code.render::<Luma<u8>>()
            .min_dimensions(200, 200)
            .max_dimensions(200, 200)
            .build();
            
        image.save(output_path)?;
        println!("QR code saved to: {}", output_path);
        Ok(())
    }
    
    pub fn qr_to_binary(&self, qr_path: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // TODO: Use rqrr library for proper QR decoding
        
        // This is a simple test that "simulates" QR decoding
        // by storing the original data in a companion file
        let companion_path = format!("{}.data", qr_path);
        let data = fs::read(&companion_path)?;
        
        // TODO
        // 1. Load the QR image
        // 2. Use a QR decoding library to extract the base64 data
        // 3. Decode base64 back to binary
        
        Ok(data)
    }
    
    pub fn export_files_as_qr(&self, output_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(output_dir)?;
        
        for (inode, file) in &self.files {
            if file.attrs.kind == FileType::RegularFile {
                if let Some(content) = &file.data {
                    let binary_data = content.as_bytes();
                    let qr_path = format!("{}/file_{}.png", output_dir, inode);
                    
                    self.binary_to_qr(binary_data, &qr_path)?;
                    
                    // For testing, also save the original data as companion file
                    let companion_path = format!("{}.data", qr_path);
                    fs::write(&companion_path, binary_data)?;
                    
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
                // (using our companion file for now)
                if let Some(binary_data) = self.extract_data_from_qr_path(&path) {
                    let file_name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .replace("file_", "imported_");
                    
                    let content = String::from_utf8_lossy(&binary_data).to_string();
                    
                    self.push(file_name, Some(content), Some(parent_inode), false);
                    println!("Imported file from QR: {}", path.display());
                }
            }
        }
        Ok(())
    }
    
    fn extract_data_from_qr_path(&self, qr_path: &std::path::Path) -> Option<Vec<u8>> {
        // For now, read from companion file
        let companion_path = format!("{}.data", qr_path.display());
        fs::read(&companion_path).ok()
    }

}


impl Filesystem for QRFileSystem {

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        println!("getattr(ino={})", ino);
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


    // fn write(&mut self, _req: &Request, ino: u64, fh: u64, offset: i64, data: &[u8], write_flags: u32, flags: i32, lock_owner: Option<u64>, reply: ReplyWrite) {

    // }


    // fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {

    // }


    // fn create(&mut self, _req: &Request, parent: u64, name: &OsStr, mode: u32, umask: u32, flags: i32, reply: ReplyCreate) {

    // }


    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let file_name = name.to_str().unwrap().to_string();
        self.delete_file(parent, file_name);
        reply.ok();
    }


    // fn fsyncdir(&mut self, _req: &Request, ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {

    // }


    fn read(&mut self, _req: &Request, ino: u64, fh: u64, offset: i64, size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        println!("read(ino={}, fh={}, offset={}, size={})", ino, fh, offset, size);

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

        reply.data(data.as_bytes()); // If data wasn't a string, you have to apply pretty() and to_string() before apply as_bytes()
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
    fs.push("fileA.txt".to_string(), Some("Content A".to_string()), Some(1), false);
    fs.push("fileB.txt".to_string(), Some("Content B".to_string()), Some(1), false);
    fs.push("fileC.txt".to_string(), Some("Binary-like data: \x00\x01\x02\x03".to_string()), Some(1), false);

    println!("Testing QR code generation...");
    
    let test_dir = "./qr_test";
    if let Err(e) = fs.export_files_as_qr(test_dir) {
        eprintln!("QR export failed: {}", e);
    }

    println!("\nTesting QR code import...");
    if let Err(e) = fs.import_files_from_qr(test_dir, 1) {
        eprintln!("QR import failed: {}", e);
    }

    println!("\nTesting with actual binary data...");
    let binary_data = vec![0x48, 0x65, 0x6C, 0x6C, 0x6F, 0x20, 0x57, 0x6F, 0x72, 0x6C, 0x64]; // "Hello World"
    if let Err(e) = fs.binary_to_qr(&binary_data, "./test_binary_qr.png") {
        eprintln!("Binary QR test failed: {}", e);
    }

    let mountpoint = match env::args().nth(1) {
        Some(p) => p,
        None => {
            println!("Usage: <program> <MOUNTPOINT>");
            return;
        }
    };

    println!("\nMounting filesystem at: {}", mountpoint);
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
