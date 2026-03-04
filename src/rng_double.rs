// mypaint-rs: rng_double.rs
// Rewrite of libmypaint/rng-double.c
// Original algorithm by D.E. Knuth (public domain)
// C port by Jon Nordby (public domain)
// Rust port: see original headers for full credits

// ── Constants ────────────────────────────────────────────────────────────────

#[cfg(feature = "original")]
const QUALITY: usize = 1009;
#[cfg(feature = "original")]
const TT: usize = 70;
#[cfg(feature = "original")]
const KK: usize = 100;
#[cfg(feature = "original")]
const LL: usize = 37;

/// 低质量设定，足够 MyPaint 使用
#[cfg(not(feature = "original"))]
const QUALITY: usize = 19;
#[cfg(not(feature = "original"))]
const TT: usize = 7;
#[cfg(not(feature = "original"))]
const KK: usize = 10;
#[cfg(not(feature = "original"))]
const LL: usize = 7;

// ── Internal helpers ─────────────────────────────────────────────────────────

#[inline(always)]
fn is_odd(s: u64) -> bool {
    s & 1 == 1
}

/// `(x + y) mod 1.0`，等价于 C 的 `(x+y) - (int)(x+y)`。
/// 由于调用方保证 x, y ∈ [0, 1)，x+y ∈ [0, 2)，floor 与截断结果相同。
#[inline(always)]
fn mod_sum(x: f64, y: f64) -> f64 {
    let s = x + y;
    s - s.floor()
}

// ── RngDouble ────────────────────────────────────────────────────────────────

/// Knuth 的滞后 Fibonacci 伪随机数生成器（TAOCP Vol.2, §3.6）。
///
/// 产生均匀分布在 [0, 1) 上的 `f64` 序列。每个实例相互独立。
#[derive(Clone)]
pub struct RngDouble {
    /// 生成器内部状态，长度 KK
    rng_state: [f64; KK],
    /// 预生成缓冲区，长度 QUALITY + 1（+1 用于存放哨兵 -1.0）
    buffer: [f64; QUALITY + 1],
    /// 下一个待返回值在 buffer 中的下标；None 表示尚未初始化
    index: Option<usize>,
}

impl RngDouble {
    /// 以给定种子创建一个新的生成器实例。
    pub fn new(seed: u64) -> Self {
        // 用 0 填充，随后 set_seed 会完整初始化
        let mut rng = Self {
            rng_state: [0.0; KK],
            buffer: [0.0; QUALITY + 1],
            index: None,
        };
        rng.set_seed(seed);
        rng
    }

    /// 重新以给定种子初始化生成器（等价于 C 的 `rng_double_set_seed`）。
    pub fn set_seed(&mut self, seed: u64) {
        debug_assert!(QUALITY > KK, "QUALITY must be > KK for the sentinel to work");

        const ULP: f64 = (1.0 / (1u64 << 30) as f64) / (1u64 << 22) as f64; // 2^-52

        let mut u = [0.0f64; KK + KK - 1];
        let mut ss = 2.0 * ULP * ((seed & 0x3fff_ffff) + 2) as f64;

        // 自举缓冲区
        for j in 0..KK {
            u[j] = ss;
            ss += ss;
            if ss >= 1.0 {
                ss -= 1.0 - 2.0 * ULP;
            }
        }
        u[1] += ULP; // 让 u[1]（且仅 u[1]）为"奇数"

        let mut s = seed & 0x3fff_ffff;
        let mut t = TT - 1;
        loop {
            // "平方"
            for j in (1..KK).rev() {
                u[j + j] = u[j];
                u[j + j - 1] = 0.0;
            }
            for j in (KK..=(KK + KK - 2)).rev() {
                u[j - (KK - LL)] = mod_sum(u[j - (KK - LL)], u[j]);
                u[j - KK] = mod_sum(u[j - KK], u[j]);
            }
            // "乘以 z"
            if is_odd(s) {
                for j in (1..=KK).rev() {
                    u[j] = u[j - 1];
                }
                u[0] = u[KK];
                u[LL] = mod_sum(u[LL], u[KK]);
            }
            if s != 0 {
                s >>= 1;
            } else {
                if t == 0 { break; }
                t -= 1;
            }
        }

        // 将 u 写入 rng_state
        self.rng_state[KK - LL..KK].copy_from_slice(&u[0..LL]);
        self.rng_state[0..KK - LL].copy_from_slice(&u[LL..KK]);

        // 热身 10 次
        for _ in 0..10 {
            self.get_array(&mut u, KK + KK - 1);
        }

        self.index = None; // 标记为"已初始化，等待第一次 cycle"
    }

    /// 填充 `array[0..n]`，同时更新内部状态。
    ///
    /// **必须使用顺序循环**：后面的项依赖前面刚写入的值，不可并行化。
    fn get_array(&mut self, array: &mut [f64], n: usize) {
        // 将当前状态复制到 array 前 KK 位
        array[0..KK].copy_from_slice(&self.rng_state);

        // 顺序生成 array[KK..n]（每项读取自身 array 中刚写入的值）
        for i in KK..n {
            array[i] = mod_sum(array[i - KK], array[i - LL]);
        }

        // 更新 rng_state 前 LL 项
        for i in 0..LL {
            self.rng_state[i] = mod_sum(array[n + i - KK], array[n + i - LL]);
        }
        // 更新 rng_state 后 KK-LL 项（读取已更新的 rng_state[i-LL]）
        for i in LL..KK {
            self.rng_state[i] = mod_sum(array[n + i - KK], self.rng_state[i - LL]);
        }
    }

    /// 重新填充缓冲区并返回 `buffer[0]`（等价于 C 的 `rng_double_cycle`）。
    fn cycle(&mut self) -> f64 {
        // get_array 需要一个 &mut [f64]；用临时数组避免借用冲突
        let mut tmp = [0.0f64; QUALITY + 1];
        self.get_array(&mut tmp, QUALITY);
        tmp[KK] = -1.0; // 哨兵：buffer[KK] 是第一个"越界"位置
        self.buffer = tmp;
        self.index = Some(1); // 下次 next() 从 buffer[1] 开始
        self.buffer[0]
    }

    /// 返回下一个随机数，范围 [0, 1)。
    pub fn next(&mut self) -> f64 {
        match self.index {
            Some(i) => {
                let v = self.buffer[i];
                if v >= 0.0 {
                    self.index = Some(i + 1);
                    v
                } else {
                    // 遇到哨兵，重新填充
                    self.cycle()
                }
            }
            None => self.cycle(),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 所有输出必须落在 [0, 1)
    #[test]
    fn test_range() {
        let mut rng = RngDouble::new(12345);
        for _ in 0..10_000 {
            let v = rng.next();
            assert!(v >= 0.0 && v < 1.0, "out of range: {v}");
        }
    }

    /// 相同种子产生相同序列（确定性）
    #[test]
    fn test_deterministic() {
        let mut a = RngDouble::new(42);
        let mut b = RngDouble::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next().to_bits(), b.next().to_bits());
        }
    }

    /// 不同种子产生不同序列
    #[test]
    fn test_different_seeds() {
        let mut a = RngDouble::new(1);
        let mut b = RngDouble::new(2);
        let differs = (0..100).any(|_| a.next().to_bits() != b.next().to_bits());
        assert!(differs, "seeds 1 and 2 produced identical sequences");
    }

    /// set_seed 可重置生成器到与 new() 相同的状态
    #[test]
    fn test_reseed() {
        let mut rng = RngDouble::new(99);
        let first: Vec<u64> = (0..200).map(|_| rng.next().to_bits()).collect();

        rng.set_seed(99);
        let second: Vec<u64> = (0..200).map(|_| rng.next().to_bits()).collect();

        assert_eq!(first, second);
    }

    /// 跨缓冲区边界（QUALITY 次调用后）连续性
    #[test]
    fn test_across_buffer_boundary() {
        let mut rng = RngDouble::new(7);
        // 消耗超过一个完整 buffer，确保 cycle() 被触发多次
        for _ in 0..(QUALITY * 3 + 5) {
            let v = rng.next();
            assert!(v >= 0.0 && v < 1.0);
        }
    }
}