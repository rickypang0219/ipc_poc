// 共用 crate：feed 和 sink 兩個 binary 都依賴它，保證兩邊的 memory layout 一模一樣
use std::sync::atomic::{AtomicU64, Ordering::*};
use std::{
    fs::OpenOptions,
    os::fd::AsRawFd,
    ptr::{addr_of, addr_of_mut},
    thread,
    time::Duration,
};

#[cfg(target_os = "linux")]
const POPULATE: libc::c_int = libc::MAP_POPULATE; // Linux 專有：map 時預先建好 page table
#[cfg(not(target_os = "linux"))]
const POPULATE: libc::c_int = 0; // macOS 等平台沒有，靠下面的逐頁讀取代替

pub const CAP: u64 = 1 << 16;
const MASK: u64 = CAP - 1;
// magic 同時是 layout 版本：改了 Msg / CAP 就要改這個數，舊 process 便不會誤讀
const MAGIC: u64 = 0x4D44_5249_4E47_0002 ^ (std::mem::size_of::<Msg>() as u64) ^ CAP;

#[repr(C, align(64))]
struct Padded(AtomicU64);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Msg {
    pub seq: u64,
    pub ts_ns: u64,
    pub px_ticks: i64,
    pub qty: i64,
    pub sym_id: u32,
    pub side: u8,
    _p: [u8; 3],
}

impl Msg {
    pub fn new(seq: u64, ts_ns: u64, sym_id: u32, side: u8, px_ticks: i64, qty: i64) -> Self {
        Msg {
            seq,
            ts_ns,
            px_ticks,
            qty,
            sym_id,
            side,
            _p: [0; 3],
        }
    }
}

#[repr(C)]
struct Layout {
    magic: Padded,
    head: Padded,
    tail: Padded,
    slots: [Msg; CAP as usize],
}

/// 一個 mapping 到 shared memory 的 ring。head/tail 存在 shm 裡，
/// 所以任何一方重啟都可以由上次的位置繼續（只要 tmpfs volume 還在）。
pub struct Ring(*mut Layout);
unsafe impl Send for Ring {}

pub fn now_ns() -> u64 {
    // CLOCK_REALTIME：同一 kernel 下所有 container 共用，亦可直接做 DB timestamp
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

impl Ring {
    /// producer：檔案不存在就建立；consumer：等到 producer 初始化完成才返回
    pub fn open(path: &str, producer: bool) -> Ring {
        let size = std::mem::size_of::<Layout>();
        let f = loop {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create(producer)
                .open(path)
            {
                Ok(f) => break f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // 只有「未出現」才等
                    eprintln!("waiting for {path} ...");
                    thread::sleep(Duration::from_millis(500))
                }
                Err(e) => panic!("cannot open {path}: {e} (check volume uid / permissions)"),
            }
        };
        if producer && f.metadata().unwrap().len() != size as u64 {
            f.set_len(size as u64).unwrap();
        }
        // consumer 必須等 producer set_len 完成才 mmap：
        // 檔案長度不足時，存取超出 EOF 的 page 會觸發無法處理的 page fault → SIGBUS
        while !producer && f.metadata().unwrap().len() < size as u64 {
            thread::sleep(Duration::from_millis(100));
        }
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | POPULATE,
                f.as_raw_fd(),
                0,
            )
        }; // 必須 SHARED，不可 PRIVATE
        assert!(p != libc::MAP_FAILED, "mmap failed");
        for off in (0..size).step_by(4096) {
            // 逐頁「讀」一次預熱 page table；讀不會破壞對方已寫的資料
            unsafe { std::ptr::read_volatile((p as *const u8).add(off)) };
        }
        if unsafe { libc::mlock(p, size) } != 0 {
            eprintln!("warn: mlock failed (raise memlock ulimit)");
        }
        let r = Ring(p as *mut Layout);
        let magic = unsafe { &(*r.0).magic.0 };
        if producer && magic.load(Acquire) != MAGIC {
            // 全新（或舊版本）檔案才初始化
            unsafe {
                (*r.0).head.0.store(0, Relaxed);
                (*r.0).tail.0.store(0, Relaxed);
            }
            magic.store(MAGIC, Release); // 最後才公佈
            eprintln!("ring initialised at {path}");
        }
        while magic.load(Acquire) != MAGIC {
            thread::sleep(Duration::from_millis(100));
        }
        eprintln!("mapped {path} at virtual addr {p:p}");
        r
    }

    pub fn push(&self, m: Msg) -> bool {
        // 只可由 producer 呼叫
        unsafe {
            let h = (*self.0).head.0.load(Relaxed);
            if h - (*self.0).tail.0.load(Acquire) == CAP {
                return false;
            } // 滿：丟棄，絕不 block
            addr_of_mut!((*self.0).slots[(h & MASK) as usize]).write(m);
            (*self.0).head.0.store(h + 1, Release);
            true
        }
    }

    pub fn pop(&self) -> Option<Msg> {
        // 只可由 consumer 呼叫
        unsafe {
            let t = (*self.0).tail.0.load(Relaxed);
            if t == (*self.0).head.0.load(Acquire) {
                return None;
            }
            let m = addr_of!((*self.0).slots[(t & MASK) as usize]).read();
            (*self.0).tail.0.store(t + 1, Release);
            Some(m)
        }
    }
}

pub fn env_or<T: std::str::FromStr>(k: &str, d: T) -> T {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}
