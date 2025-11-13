extern crate libc;
extern crate time;

use std::path::Path;
use libc::{ENOENT, ENOSYS};
// use time::Timespec;
use libc::timespec;

use std::env;
use fuser::{FileAttr, FileType, Filesystem, Request, ReplyAttr, ReplyData, ReplyEntry, ReplyDirectory};

struct JsonFilesystem;

impl Filesystem for JsonFilesystem {
    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        println!("getattr(ino={})", ino);
        reply.error(ENOSYS);
    }
}

fn main() { 
    let mountpoint = match env::args().nth(1) {
        Some(path) => path,
        None => {
            println!("Usage: {} <MOUNTPOINT>", env::args().nth(0).unwrap());
            return;
        }
    };
    let _ = fuser::mount2(JsonFilesystem, &mountpoint, &[]);
}

// To unmount, run the command: fusermount -u /tmp/rust-fuse
// if you don't unmount, you'll run into errors next time you try cargo run.
