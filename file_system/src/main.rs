extern crate libc;
extern crate time;

use std::path::Path;
use libc::{ENOENT, ENOSYS};
// use time::Timespec;
use libc::timespec;
use std::time::{SystemTime, Duration, UNIX_EPOCH};

use std::env;
use fuser::{FileAttr, FileType, Filesystem, Request, ReplyAttr, ReplyData, ReplyEntry, ReplyDirectory};

fn timespec_to_system_time(ts: timespec) -> SystemTime {
    UNIX_EPOCH + Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

struct JsonFilesystem;

impl Filesystem for JsonFilesystem {
    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        println!("getattr(ino={})", ino);
        let ts = timespec_to_system_time(timespec { tv_sec: 0, tv_nsec: 0 }); 
        let attr = FileAttr {
            ino: 1,
            size: 0,
            blocks: 0,
            atime: ts,
            mtime: ts,
            ctime: ts,
            crtime: ts,
            kind: FileType::Directory,
            perm: 0o755,
            nlink: 0,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 4096 // This is the size of the E/S blocks in bytes that the file system uses for that dir or file.
        };
        let ttl = timespec_to_system_time(timespec { tv_sec: 1, tv_nsec: 0 });
        let ttl_duration = ttl.duration_since(UNIX_EPOCH).unwrap();
        if ino == 1 {
            reply.attr(&ttl_duration, &attr);
        } else {
            reply.error(ENOSYS);
        }
    }

    fn readdir(&mut self, _req: &Request, ino: u64, fh: u64, offset: i64, mut reply: ReplyDirectory) {
        println!("readdir(ino={}, fh={}, offset={})", ino, fh, offset);
        if ino == 1 {
            if offset == 0 {
                let _ = reply.add(1, 0, FileType::Directory, &Path::new("."));
                let _ = reply.add(1, 1, FileType::Directory, &Path::new(".."));
            }
            reply.ok();
        } else {
            reply.error(ENOENT);
        }
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
