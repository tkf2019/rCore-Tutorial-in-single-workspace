use alloc::sync::Arc;
use alloc::{string::String, vec::Vec};
use easy_fs::{EasyFileSystem, FSManager, FileHandle, Inode, OpenFlags};
use lazy_static::*;

use crate::virtio_block::BLOCK_DEVICE;

pub struct FileSystem {
    root: Arc<Inode>,
}

impl FSManager for FileSystem {
    fn open(&self, path: &str, flags: OpenFlags) -> Option<Arc<FileHandle>> {
        let (readable, writable) = flags.read_write();
        if flags.contains(OpenFlags::CREATE) {
            if let Some(inode) = self.find(path) {
                // Clear size
                inode.clear();
                Some(Arc::new(FileHandle::new(readable, writable, inode)))
            } else {
                // Create new file
                self.root
                    .create(path)
                    .map(|new_inode| Arc::new(FileHandle::new(readable, writable, new_inode)))
            }
        } else {
            self.find(path).map(|inode| {
                if flags.contains(OpenFlags::TRUNC) {
                    inode.clear();
                }
                Arc::new(FileHandle::new(readable, writable, inode))
            })
        }
    }

    fn find(&self, path: &str) -> Option<Arc<Inode>> {
        self.root.find(path)
    }

    fn readdir(&self, _path: &str) -> Option<alloc::vec::Vec<String>> {
        Some(self.root.readdir())
    }

    fn link(&self, _src: &str, _dst: &str) -> isize {
        unimplemented!()
    }

    fn unlink(&self, _path: &str) -> isize {
        unimplemented!()
    }
}

impl FileSystem {
    fn new() -> Self {
        let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
        Self {
            root: Arc::new(EasyFileSystem::root_inode(&efs)),
        }
    }
}

lazy_static! {
    pub static ref FS: Arc<FileSystem> = Arc::new(FileSystem::new());
}

pub fn read_all(fd: Arc<FileHandle>) -> Vec<u8> {
    let mut offset = 0usize;
    let mut buffer = [0u8; 512];
    let mut v: Vec<u8> = Vec::new();
    if let Some(inode) = &fd.inode {
        loop {
            let len = inode.read_at(offset, &mut buffer);
            if len == 0 {
                break;
            }
            offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
    }
    v
}
