// Hot path：模擬 100 個 symbol 的 book update，寫入 shm ring。
// 真實版這裡會是 Binance WS → parse → 更新 book → UDP multicast；寫 ring 只是旁路。
use ring::{env_or, now_ns, Msg, Ring};

fn main() {
    let path: String = env_or("RING_PATH", "/ring/md".to_string());
    let gap_ns: u64 = env_or("GAP_NS", 2_000);        // 每條 update 間隔（模擬 feed 速率）
    let count: u64 = env_or("COUNT", 0);              // 0 = 永遠跑
    let ring = Ring::open(&path, true);

    let mut px: Vec<i64> = (0..100).map(|i| 1_000_000 + i * 10_000).collect();   // 預先分配
    let (mut rng, mut seq, mut drops) = (0x9E37_79B9_7F4A_7C15u64, 0u64, 0u64);
    let mut next_report = now_ns() + 5_000_000_000;

    while count == 0 || seq < count {
        let target = now_ns() + gap_ns;
        while now_ns() < target { std::hint::spin_loop(); }   // busy-spin，不 sleep

        rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17;  // xorshift：零 allocation 的亂數
        let sym = (rng % 100) as usize;
        px[sym] += (rng >> 8) as i64 % 5 - 2;                // random walk ±2 ticks
        let m = Msg::new(seq, now_ns(), sym as u32, (rng >> 20 & 1) as u8, px[sym], ((rng >> 24) % 1000) as i64 + 1);
        if !ring.push(m) { drops += 1; }
        seq += 1;

        if now_ns() > next_report {                          // 非 hot path 的 I/O，5 秒一次
            eprintln!("feed: sent {seq}, dropped {drops}");
            next_report += 5_000_000_000;
        }
    }
    eprintln!("feed: done, sent {seq}, dropped {drops}");
}
