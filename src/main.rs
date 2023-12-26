use gmp_mpfr_sys::gmp;
use std::{
    cmp::Ordering,
    env,
    fs::{File, OpenOptions},
    hint,
    io::{prelude::*, BufWriter, LineWriter},
    iter,
    sync::atomic::{self, AtomicI64, AtomicU64, AtomicUsize, Ordering::*},
    thread,
    time::Duration,
};

const _: gmp::limb_t = 0_u64; // assert 64-bit limb size

#[repr(C, align(4096))]
struct Align4096<T>(T);

#[rustfmt::skip]
const TABLE: Align4096<[(i32, u32); 81]> = Align4096([
    (1, 2), (1, 5), (-1, 9), (2, 11), (0, 15), (-2, 19), (1, 21), (1, 24),
    (2, 27), (2, 30), (0, 34), (0, 37), (3, 39), (1, 43), (-1, 47), (2, 49),
    (0, 53), (3, 55), (3, 58), (1, 62), (-1, 66), (-1, 69), (2, 71), (0, 75),
    (3, 77), (1, 81), (-1, 85), (2, 87), (2, 90), (0, 94), (0, 97), (-2, 101),
    (-1, 104), (-1, 107), (2, 109), (0, 113), (3, 115), (1, 119), (1, 122),
    (1, 125), (-1, 129), (0, 132), (0, 135), (-2, 139), (1, 141), (4, 143),
    (2, 147), (0, 151), (0, 154), (0, 157), (1, 160), (1, 163), (-1, 167),
    (0, 170), (0, 173), (3, 175), (1, 179), (1, 182), (-1, 186), (0, 189),
    (0, 192), (0, 195), (1, 198), (1, 201), (-1, 205), (2, 207), (2, 210),
    (0, 214), (1, 217), (1, 220), (-1, 224), (2, 226), (2, 229), (0, 233),
    (-2, 237), (1, 239), (1, 242), (2, 245), (2, 248), (0, 252), (1, 255),
]);

fn cmp_wide(x: &[u64], y: &[u64]) -> Ordering {
    if x.len() < y.len() {
        return Ordering::Less;
    }
    if x.len() > y.len() {
        return Ordering::Greater;
    }
    iter::zip(x, y)
        .rev()
        .map(|(x, y)| x.cmp(y))
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

fn step_16(mut end: u128, a: &mut i64) -> u128 {
    for _ in 0..16 {
        let rem = end % 81;
        end /= 81;
        let (a_off, b_off) = TABLE.0[rem as usize];
        *a += a_off as i64;
        assert!(*a >= 2, "a miracle occurred");
        end = (end << 8) + b_off as u128;
    }
    end
}

struct State {
    i: u64,
    a: i64,
    buffer: Vec<u64>,
    pow81: Vec<Box<[u64]>>,
    log_file: BufWriter<LineWriter<File>>,
    last_end: u128,
}

impl State {
    fn new(log_file: File) -> Self {
        let pow1 = Box::new([81_u64.pow(8)]);
        let pow2 = Box::new([81_u128.pow(16) as u64, (81_u128.pow(16) >> 64) as u64]);
        Self {
            i: 0_u64,
            a: 2_i64,
            buffer: vec![0_u64; 5],
            pow81: vec![pow1, pow2],
            log_file: BufWriter::new(LineWriter::new(log_file)),
            last_end: 0_u128,
        }
    }
}

struct Stats {
    i2: AtomicU64,
    level: AtomicUsize,
    a: AtomicI64,
    end0: AtomicU64,
    end1: AtomicU64,
}

impl Stats {
    fn new() -> Self {
        Self {
            i2: AtomicU64::new(0),
            level: AtomicUsize::new(0),
            a: AtomicI64::new(2),
            end0: AtomicU64::new(0),
            end1: AtomicU64::new(0),
        }
    }
}

enum Seq {
    First,
    Second,
}

fn step_wide_level0(state: &mut State, stats: &Stats, start: usize) {
    stats.level.store(0, Relaxed);
    let buf = &mut state.buffer[start..];
    let end = buf[0] as u128 | (buf[1] as u128) << 64;
    let State { i, a, last_end, .. } = *state;
    writeln!(&mut state.log_file, "{i} {a} {last_end} {end}").unwrap();
    let end = step_16(end, &mut state.a);
    state.i += 1;
    state.last_end = end;
    buf[2] = end as u64;
    buf[3] = (end >> 64) as u64;
    stats.i2.store(2 * state.i - 1, Relaxed);
    atomic::fence(Release);
    stats.a.store(state.a, Relaxed);
    stats.end0.store(buf[2], Relaxed);
    stats.end1.store(buf[3], Relaxed);
    atomic::fence(Release);
    stats.i2.store(2 * state.i, Relaxed);
}

fn step_wide(state: &mut State, stats: &Stats, level: usize, seq: Seq, start: usize) {
    assert!(level != 0);
    stats.level.store(level, Relaxed);
    let buf = &mut state.buffer[start..];
    let (len256, len81) = (1_usize << level, state.pow81[level].len());
    let (num, quot) = buf.split_at_mut(2 * len256);
    let (num_len, quot_len) = match seq {
        Seq::First => (state.pow81[level + 1].len(), len81),
        Seq::Second => (len256 + len81, len256),
    };
    let saved = quot[quot_len]; // limb is clobbered by mpn_tdiv_qr()
    let (num_ptr, quot_ptr) = (num.as_mut_ptr(), quot.as_mut_ptr());
    let den_ptr = state.pow81[level].as_ptr();
    unsafe {
        gmp::mpn_tdiv_qr(
            quot_ptr,
            num_ptr,
            0,
            num_ptr,
            num_len as gmp::size_t,
            den_ptr,
            len81 as gmp::size_t,
        );
    }
    quot[quot_len] = saved;
    if level > 1 {
        step_wide(state, stats, level - 1, Seq::First, start);
        step_wide(state, stats, level - 1, Seq::Second, start + len256 / 2);
    } else {
        step_wide_level0(state, stats, start);
    }
}

fn main_loop(state: &mut State, stats: &Stats) {
    loop {
        let level = state.pow81.len() - 1;
        stats.level.store(level, Relaxed);
        let (len256, len81) = (1_usize << level, state.pow81[level].len());
        let mut top = len256;
        while top >= len81 && state.buffer[top - 1] == 0 {
            top -= 1;
        }
        if cmp_wide(&state.buffer[..top], &state.pow81[level]).is_lt() {
            if level > 1 {
                step_wide(state, stats, level - 1, Seq::First, 0);
                step_wide(state, stats, level - 1, Seq::Second, len256 / 2);
            } else {
                step_wide_level0(state, stats, 0);
            }
            let (dst, src) = state.buffer.split_at_mut(len256);
            dst.copy_from_slice(&src[..len256]);
        } else {
            let mut dst: Vec<u64> = Vec::with_capacity(2 * len81);
            let (src_ptr, dst_ptr) = (state.pow81[level].as_ptr(), dst.as_mut_ptr());
            unsafe {
                gmp::mpn_sqr(dst_ptr, src_ptr, len81 as gmp::size_t);
                dst.set_len(2 * len81);
            }
            if *dst.last().unwrap() == 0 {
                dst.pop();
            }
            state.pow81.push(dst.into_boxed_slice());
            state.buffer[len256..2 * len256].fill(0);
            state.buffer.resize(4 * len256 + 1, 0);
        }
    }
}

fn status_loop(stats: &Stats) {
    let mut last_i2 = 0;
    let mut last_level = 0;
    let mut level_secs = 0_usize;
    loop {
        let mut i2 = stats.i2.load(Relaxed);
        let (mut level, mut a, mut end0, mut end1);
        loop {
            while i2 % 2 != 0 {
                hint::spin_loop();
                i2 = stats.i2.load(Relaxed);
            }
            atomic::fence(Acquire);
            level = stats.level.load(Relaxed);
            a = stats.a.load(Relaxed);
            end0 = stats.end0.load(Relaxed);
            end1 = stats.end1.load(Relaxed);
            atomic::fence(Acquire);
            let next_i2 = stats.i2.load(Relaxed);
            if next_i2 == i2 {
                break;
            }
            i2 = next_i2;
        }
        if i2 == last_i2 {
            if level != last_level {
                level_secs = 0;
            }
            let pow = 8_u64 << level;
            if level_secs == 0 {
                println!("iter {}: level {level}, den = 81^{pow}", i2 * 4 * 8);
            } else {
                println!(
                    "iter {}: level {level}, den = 81^{pow} ({level_secs}s)",
                    i2 * 4 * 8
                );
            }
            level_secs += 1;
        } else {
            let end = (end1 as u128) << 64 | end0 as u128;
            println!("iter {}: a = {a}, b % 81^16 = {end}", i2 * 4 * 8);
            level_secs = 0;
        }
        last_i2 = i2;
        last_level = level;
        thread::sleep(Duration::from_secs(1));
    }
}

fn main() {
    let mut args = env::args_os().fuse().skip(1);
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(args.next().unwrap())
        .unwrap();
    let mut state = State::new(log_file);
    let stats = Stats::new();
    thread::scope(|s| {
        s.spawn(|| status_loop(&stats));
        main_loop(&mut state, &stats);
    });
}
