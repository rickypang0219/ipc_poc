// Cold path：從 shm ring 讀出，批次用 ILP（InfluxDB Line Protocol）經 TCP 寫入 QuestDB。
// 它慢、斷線、重啟都不會影響 feed：最壞情況只是 ring 滿了，feed 那邊 drop 並計數。
use ring::{env_or, now_ns, Ring};
use std::{io::{BufWriter, Write}, net::TcpStream, thread, time::Duration};

fn connect(addr: &str) -> BufWriter<TcpStream> {
    loop {
        match TcpStream::connect(addr) {
            Ok(s) => { eprintln!("sink: connected to {addr}"); return BufWriter::with_capacity(1 << 20, s); }
            Err(e) => { eprintln!("sink: {addr} not ready ({e}), retrying"); thread::sleep(Duration::from_secs(1)); }
        }
    }
}

fn main() {
    let path: String = env_or("RING_PATH", "/ring/md".to_string());
    let addr: String = env_or("QDB_ADDR", "questdb:9009".to_string());
    let batch: usize = env_or("BATCH", 10_000);
    let ring = Ring::open(&path, false);
    let mut out = connect(&addr);

    let (mut n, mut pending, mut gaps, mut last) = (0u64, 0usize, 0u64, None::<u64>);
    let (mut lat_max, mut lat_sum) = (0u64, 0u64);
    let mut next_report = now_ns() + 5_000_000_000;

    loop {
        let Some(m) = ring.pop() else {
            if pending > 0 {                                    // 閒下來就 flush
                if out.flush().is_err() { out = connect(&addr); }
                pending = 0;
            }
            thread::sleep(Duration::from_micros(100));          // cold path 可以 sleep，讓出 CPU
            continue;
        };
        let lat = now_ns().saturating_sub(m.ts_ns);
        lat_max = lat_max.max(lat);
        lat_sum += lat;
        if let Some(l) = last { if m.seq != l + 1 { gaps += 1; } }   // gap = feed 因 ring 滿而 drop
        last = Some(m.seq);

        let side = if m.side == 0 { 'b' } else { 'a' };
        let line = writeln!(out, "book,sym=S{:03},side={side} px={}.{:02},qty={}i,seq={}i {}",
                            m.sym_id, m.px_ticks / 100, m.px_ticks % 100, m.qty, m.seq, m.ts_ns);
        if line.is_err() { out = connect(&addr); continue; }
        n += 1;
        pending += 1;
        if pending >= batch {
            if out.flush().is_err() { out = connect(&addr); }
            pending = 0;
        }

        if now_ns() > next_report {
            let avg = if n > 0 { lat_sum / n } else { 0 };
            eprintln!("sink: wrote {n}, gaps {gaps}, ring→sink latency avg {avg}ns max {lat_max}ns");
            lat_max = 0;
            next_report += 5_000_000_000;
        }
    }
}
