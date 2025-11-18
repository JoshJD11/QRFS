extern crate rustc_serialize;

use rustc_serialize::json::{self, Json};
use std::collections::BTreeMap;
use std::path::Path;
use std::env;

use fuser::{
    FileAttr, FileType, Filesystem, Request,
    ReplyDirectory, ReplyAttr, ReplyData, ReplyEntry
};

use libc::{ENOENT};

struct JsonFilesystem {
    tree: json::Object,
    attrs: BTreeMap<u64, FileAttr>,
    inodes: BTreeMap<String, u64>,
}

impl JsonFilesystem {
    fn new(tree: &json::Object) -> JsonFilesystem {
        let mut attrs = BTreeMap::new();
        let mut inodes = BTreeMap::new();

        let attr = FileAttr {
            ino: 1,
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

        attrs.insert(1, attr);
        inodes.insert("/".to_string(), 1);

        for (i, (key, value)) in tree.iter().enumerate() {
            let attr = FileAttr {
                ino: i as u64 + 2,
                size: value.pretty().to_string().len() as u64,
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

            attrs.insert(attr.ino, attr);
            inodes.insert(key.clone(), attr.ino);
        }

        JsonFilesystem { tree: tree.clone(), attrs, inodes }
    }
}

impl Filesystem for JsonFilesystem {

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        if let Some(attr) = self.attrs.get(&ino) {
            reply.attr(&std::time::Duration::from_secs(1), attr);
        } else {
            reply.error(ENOENT);
        }
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {

        if ino != 1 {
            reply.error(ENOENT);
            return;
        }

        if offset == 0 {
            let _ = reply.add(1, 0, FileType::Directory, Path::new("."));
            let _ = reply.add(1, 1, FileType::Directory, Path::new(".."));
        }

        let mut index = offset - 2;
        if index < 0 { index = 0; }

        for (i, key) in self.tree.keys().enumerate().skip(index as usize) {
            let inode = 2 + i as u64;
            let next_offset = 3 + i as i64;
            let _ = reply.add(inode, next_offset, FileType::RegularFile, Path::new(key));
        }

        reply.ok();
    }
}

fn main() {
    let raw_json = r#"
    {
        "foo": "bar",
        "answer": 42
    }
    "#;

    let parsed = Json::from_str(raw_json).unwrap();
    let tree = parsed.as_object().unwrap().clone();

    let fs = JsonFilesystem::new(&tree);

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
