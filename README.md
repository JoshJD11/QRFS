# QRFS — QR Code File System

> A FUSE-based userspace file system that uses QR codes as its persistent storage medium.

---

## What is QRFS?

QRFS is a Linux userspace file system built in Rust that stores all its data as QR code image files. When you mount a QRFS volume, you interact with it like any normal directory — create files, write data, organize folders. When you unmount it, all changes are flushed and encoded into a set of sequentially numbered QR code images (`000.png`, `001.png`, ...). To restore the file system, those images are scanned back in — either from disk or via a companion mobile app — and the entire structure is reconstructed.

---

## How It Works

### Storage Model

The physical storage backing of QRFS is a folder of QR code images. Each image represents one 512-byte block of data. Blocks are named sequentially (`000.png`, `001.png`, etc.), and their filename serves as their index.

### Internal Structure

- **Block layout:** The virtual disk is organized in 512-byte blocks across three regions: a bitmap (tracking free/used blocks), a persistent inode counter, and a data region for serialized files.
- **Directory tree:** The file system is a directed tree. A special `DirectoryFile` at the root holds metadata for every file and directory — including each node's parent and the list of blocks it occupies.
- **Serialization:** Every file and directory is serialized into a binary format that includes its inode, a fixed-size name, POSIX attributes, access timestamps, and its content.

### Export (Unmount → QR codes)

When the file system is unmounted, QRFS serializes its state and writes it out across as many QR code images as needed, ensuring data consistency between sessions.

### Import (QR codes → Mount)

On mount, QRFS reads the QR images in index order, reassembles the binary chunks, and reconstructs the file system by re-inserting files in tree order — guaranteeing that every parent directory exists before its children are added.

### Passphrase Protection

A passphrase is required every time QRFS is created or accessed. The hashed passphrase is embedded as a delimiter inside the `DirectoryFile`. Without it, the boundary of the directory structure cannot be determined and reconstruction is impossible.

---

## Architecture

```
Kernel → fuser (libfuse) → Filesystem trait → QRFS implementation
```

QRFS uses the [`fuser`](https://docs.rs/fuser) Rust crate, which provides safe bindings to FUSE (Filesystem in Userspace). Each VFS operation (read, write, mkdir, etc.) is handled by implementing the `Filesystem` trait.

### Mobile Companion App

A React Native / Expo app is included for scanning QR codes into QRFS from a physical source (e.g., printed pages or a screen). It communicates with a local server that feeds scanned QR data directly into the file system reconstruction process.

**App screens:**
- **Home** — Usage instructions
- **Permissions** — Requests camera access
- **Scanner** — Continuously scans QR codes and POSTs each result to the local server; a "Finish" button signals end-of-scan

---

## Getting Started

### Prerequisites

- Linux (Ubuntu recommended)
- [Rust + Cargo](https://www.rust-lang.org/tools/install)
- FUSE installed on your system:
  ```bash
  sudo apt install fuse libfuse-dev
  ```

### Build

```bash
git clone https://github.com/JoshJD11/QRFS.git
cd QRFS/file_system
cargo build --release

# Compiled binaries will be in:
ls target/release/
```

---

## Usage

> All commands (except `toPDF-qrfs`) will prompt for the **passphrase** of the file system.

```bash
# Create a new QRFS volume in a folder
mkfs-qrfs qrfolder/

# Check file system consistency
fsck-qrfs qrfolder/

# Mount the file system at a mountpoint
mount-qrfs qrfolder/ mountpoint/

# Import QR codes scanned from the mobile app
lector-qrfs qrfolder/

# Export QR codes to a PDF file
toPDF-qrfs qrfolder/ output-folder/

# Unmount the file system
fusermount -u mountpoint/
```

> ⚠️ After mounting, the terminal will remain open to keep the session alive. **Do not close it forcefully.** Always unmount using `fusermount -u` to ensure data is flushed and QR codes are updated.

---

## Project Structure

```
QRFS/
└── file_system/    # Rust crate — FUSE implementation, QR encode/decode, disk layout, CLI tools
```

---

## Tech Stack

| Component | Technology |
|---|---|
| File system | Rust, [fuser](https://docs.rs/fuser) (FUSE bindings) |
| QR encoding | [`qrcode`](https://docs.rs/qrcode) crate |
| QR decoding | [`rqrr`](https://docs.rs/rqrr) crate |
| Mobile app | React Native + Expo |
| Build tool | Cargo |
| Platform | Linux (Ubuntu) |

---

## Authors

- [@JoshJD11](https://github.com/JoshJD11) — Joshua Jiménez Delgado
- [@K-lobiTo](https://github.com/K-lobiTo) — Caleb Alfaro Moreira
- [@Sebco27](https://github.com/Sebco27) — Sebastián Quesada
