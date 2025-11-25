use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const MAGIC: u64 = 0xF5F5_F5F5;

const BLOCK_COUNT: u64 = 2048; 
const BLOCK_SIZE: u64 = 512;


#[derive(Debug, Clone)]
struct SuperBlock {
    magic: u64,
    block_size: u64,
    total_blocks: u64,
    bitmap_start: u64, 
    data_start: u64,
    inode_counter: u64, // can actually be named like "cant_busy_blocks"
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

fn write_superblock(f: &mut File, sb: &SuperBlock) -> std::io::Result<()> {
    let mut off = 0u64;
    write_u64(f, off, sb.magic)?;
    off += 8;
    write_u64(f, off, sb.block_size)?;
    off += 8;
    write_u64(f, off, sb.total_blocks)?;
    off += 8;
    write_u64(f, off, sb.bitmap_start)?;
    off += 8;
    write_u64(f, off, sb.data_start)?;
    off += 8;
    write_u64(f, off, sb.inode_counter)?;

    Ok(())
}

fn read_superblock(f: &mut File) -> std::io::Result<SuperBlock> {
    let mut off = 0u64;
    let magic = read_u64(f, off)?;
    off += 8;
    let block_size = read_u64(f, off)?;
    off += 8;
    let total_blocks = read_u64(f, off)?;
    off += 8;
    let bitmap_start = read_u64(f, off)?;
    off += 8;
    let data_start = read_u64(f, off)?;
    off += 8;
    let inode_counter = read_u64(f, off)?;

    Ok(SuperBlock {
        magic,
        block_size,
        total_blocks,
        bitmap_start,
        data_start,
        inode_counter,
    })
}

fn read_bitmap(f: &mut File, sb: &SuperBlock) -> std::io::Result<Vec<u8>> {
    let bitmap_bytes = sb.block_size as usize;
    let mut buf = vec![0u8; bitmap_bytes];
    let offset = sb.bitmap_start * sb.block_size; 
    f.seek(SeekFrom::Start(offset))?;
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn write_bitmap(f: &mut File, sb: &SuperBlock, bitmap: &[u8]) -> std::io::Result<()> {
    let offset = sb.bitmap_start * sb.block_size;
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

fn allocate_block(f: &mut File, sb: &SuperBlock) -> std::io::Result<Option<u64>> {
    let mut bitmap = read_bitmap(f, sb)?;
    for block in sb.data_start..sb.total_blocks {
        if !bitmap_get(&bitmap, block) {
            bitmap_set_bit(&mut bitmap, block);
            write_bitmap(f, sb, &bitmap)?;
            return Ok(Some(block));
        }
    }
    Ok(None)
}

fn free_block(f: &mut File, sb: &SuperBlock, block_idx: u64) -> std::io::Result<()> {
    let mut bitmap = read_bitmap(f, sb)?;
    bitmap_clear_bit(&mut bitmap, block_idx);
    write_bitmap(f, sb, &bitmap)?;
    Ok(())
}

fn write_block(f: &mut File, sb: &SuperBlock, block_idx: u64, data: &[u8]) -> std::io::Result<()> {
    if data.len() as u64 > sb.block_size {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "data too large for block",
        ));
    }
    let offset = block_idx * sb.block_size;
    f.seek(SeekFrom::Start(offset))?;
    f.write_all(data)?;
    let pad = (sb.block_size as usize).saturating_sub(data.len());
    if pad > 0 {
        let zeros = vec![0u8; pad];
        f.write_all(&zeros)?;
    }
    Ok(())
}

fn read_block(f: &mut File, sb: &SuperBlock, block_idx: u64) -> std::io::Result<Vec<u8>> {
    let offset = block_idx * sb.block_size;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; sb.block_size as usize];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn initialize_new_disk(path: &str) -> std::io::Result<()> {
    let mut f = open_disk(path)?;

    let sb = SuperBlock {
        magic: MAGIC,
        block_size: BLOCK_SIZE,
        total_blocks: BLOCK_COUNT,
        bitmap_start: 1,
        data_start: 2, 
        inode_counter: 0, 
    };

    let total_size = BLOCK_COUNT * BLOCK_SIZE;
    f.set_len(total_size)?;

    write_superblock(&mut f, &sb)?;

    let mut bitmap = vec![0u8; BLOCK_SIZE as usize];
    bitmap_set_bit(&mut bitmap, 0); 
    bitmap_set_bit(&mut bitmap, 1); 
    write_bitmap(&mut f, &sb, &bitmap)?;

    f.sync_all()?;
    println!("Disco inicializado: '{}' ({} bytes)", path, total_size);
    Ok(())
}
