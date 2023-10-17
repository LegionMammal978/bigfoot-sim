use gmp_mpfr_sys::gmp::{self, mpz_t};
use std::{
    fs::OpenOptions,
    io::{self, prelude::*, BufWriter},
    mem::{self, MaybeUninit},
    os::raw::c_ulong,
    ptr::NonNull,
};

const fn exp81(mut x: u64) -> u64 {
    let mut res = 1;
    while x > 0 {
        res *= 81;
        x -= 1;
    }
    res
}

#[repr(align(4096))]
struct Align4096<T>(T);

#[rustfmt::skip]
const TABLE: Align4096<[(i32, u32); 81]> = Align4096([
    (1, 4), (0, 8), (1, 10), (0, 14), (-1, 18), (-2, 22), (2, 22), (1, 26),
    (0, 30), (4, 30), (-2, 38), (2, 38), (1, 42), (0, 46), (1, 48), (0, 52),
    (2, 54), (0, 58), (2, 60), (1, 64), (0, 68), (1, 70), (0, 74), (-1, 78),
    (3, 78), (0, 84), (1, 86), (0, 90), (-1, 94), (3, 94), (2, 98), (1, 102),
    (0, 106), (1, 108), (3, 110), (-1, 116), (3, 116), (0, 122), (1, 124),
    (0, 128), (-1, 132), (0, 134), (-1, 138), (1, 140), (2, 142), (1, 146),
    (0, 150), (-1, 154), (3, 154), (2, 158), (1, 162), (2, 164), (-1, 170),
    (0, 172), (2, 174), (1, 178), (2, 180), (1, 184), (0, 188), (-1, 192),
    (0, 194), (2, 196), (-2, 202), (2, 202), (-1, 208), (0, 210), (-1, 214),
    (-2, 218), (2, 218), (1, 222), (0, 226), (1, 228), (3, 230), (2, 234),
    (1, 238), (2, 240), (1, 244), (0, 248), (1, 250), (1, 254), (-1, 258),
]);

struct Integer {
    limbs: Box<[MaybeUninit<u64>]>,
    value: mpz_t,
}

impl Integer {
    fn new() -> Self {
        Self {
            limbs: Box::new([MaybeUninit::uninit(); 512]),
            value: mpz_t {
                alloc: 0,
                size: 0,
                d: NonNull::dangling(),
            },
        }
    }

    fn as_raw_mut(&mut self) -> *mut mpz_t {
        let capacity = self.limbs.len();
        let alloc = self.value.alloc as usize;
        let limbs = &mut self.limbs[capacity - alloc..];
        self.value.d = NonNull::from(limbs).cast();
        &mut self.value
    }

    fn push_limb(&mut self, value: u64) {
        let mut capacity = self.limbs.len();
        let mut alloc = self.value.alloc as usize;
        if alloc == capacity {
            self.value.alloc = self.value.size;
            alloc = self.value.alloc as usize;
            capacity *= 2;
            let mut new_limbs = Vec::with_capacity(capacity);
            unsafe { new_limbs.set_len(capacity - alloc) };
            new_limbs.extend_from_slice(&self.limbs[..alloc]);
            self.limbs = new_limbs.into_boxed_slice();
        }
        self.limbs[capacity - alloc - 1].write(value);
        self.value.alloc += 1;
        self.value.size += 1;
    }
}

fn main() -> io::Result<()> {
    const _: () = assert!(mem::size_of::<c_ulong>() == 8, "get a better OS");
    let mut log_file = BufWriter::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open("/dev/null")?,
    );
    let mut a = 2_i32;
    let mut b = Integer::new();
    let mut last_end = 0;
    'outer: for i in 0_usize.. {
        let b_raw = b.as_raw_mut();
        let mut end = unsafe { gmp::mpz_tdiv_q_ui(b_raw, b_raw, exp81(8)) };
        writeln!(&mut log_file, "{i} {a} {last_end} {end}")?;
        if i % 16 == 0 {
            println!("iter {}: a = {a}, b % 81^8 = {end}", i * 4 * 8);
        }
        for _ in 0..8 {
            let rem = end % 81;
            end /= 81;
            let (a_off, b_off) = TABLE.0[rem as usize];
            a += a_off;
            if a <= 1 {
                break 'outer;
            }
            let (tmp, ov1) = end.overflowing_shl(8);
            let (tmp, ov2) = tmp.overflowing_add(b_off as u64);
            end = tmp;
            assert!(!ov1 && !ov2, "it's odd that this never seems to overflow");
        }
        b.push_limb(end);
        last_end = end;
    }
    Ok(())
}
