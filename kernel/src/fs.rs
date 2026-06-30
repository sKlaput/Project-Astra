use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

pub type NodeId = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotMounted,
    InvalidPath,
    NotFound,
    NotFile,
    NotDirectory,
    NotEmpty,
}

#[derive(Debug, Clone, Copy)]
pub struct Mount {
    pub name: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Node {
    pub id: NodeId,
    pub name: &'static str,
    pub parent: Option<NodeId>,
    pub kind: NodeKind,
    pub data: &'static [u8],
}

#[derive(Debug, Clone, Copy)]
pub struct FileHandle {
    pub node: NodeId,
    pub offset: usize,
}

const ROOT_NODE_ID: NodeId = 0;
const ETC_NODE_ID: NodeId = 1;
const MOTD_NODE_ID: NodeId = 2;
const HELLO_NODE_ID: NodeId = 3;

static ROOT_MOUNTED: AtomicBool = AtomicBool::new(false);

static NODES: [Node; 4] = [
    Node {
        id: ROOT_NODE_ID,
        name: "",
        parent: None,
        kind: NodeKind::Directory,
        data: b"",
    },
    Node {
        id: ETC_NODE_ID,
        name: "etc",
        parent: Some(ROOT_NODE_ID),
        kind: NodeKind::Directory,
        data: b"",
    },
    Node {
        id: MOTD_NODE_ID,
        name: "motd",
        parent: Some(ETC_NODE_ID),
        kind: NodeKind::File,
        data: b"kernel vfs motd\n",
    },
    Node {
        id: HELLO_NODE_ID,
        name: "hello.txt",
        parent: Some(ROOT_NODE_ID),
        kind: NodeKind::File,
        data: b"hello from rootfs\n",
    },
];

// ── Writable buffer for /hello.txt ───────────────────────────────────────────

pub const WRITABLE_MAX: usize = 8192;
static HELLO_BUF: Mutex<[u8; WRITABLE_MAX]> = Mutex::new([0u8; WRITABLE_MAX]);
static HELLO_BUF_LEN: AtomicUsize = AtomicUsize::new(0);

include!("fs/static_vfs.rs");
include!("fs/dynamic.rs");
include!("fs/fat32_bridge.rs");
