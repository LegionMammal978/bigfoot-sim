use gmp_mpfr_sys::gmp::{self, mpz_t};
use std::{
    env,
    fs::OpenOptions,
    io::{self, prelude::*, BufWriter, LineWriter},
    mem::{self, MaybeUninit},
    os::raw::c_ulong,
    ptr::NonNull,
};

#[repr(align(4096))]
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
    (-2, 237), (1, 239), (1, 242), (2, 245), (2, 248), (0, 252), (1, 255)
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

    fn as_raw(&mut self) -> *mut mpz_t {
        let capacity = self.limbs.len();
        let alloc = self.value.alloc as usize;
        let limbs = &mut self.limbs[capacity - alloc..];
        self.value.d = NonNull::from(limbs).cast();
        &mut self.value
    }

    fn reserve(&mut self, additional: usize) {
        let mut capacity = self.limbs.len();
        let mut alloc = self.value.alloc as usize;
        if alloc + additional <= capacity {
            return;
        }
        let old_limbs = &self.limbs[capacity - alloc..];
        self.value.alloc = self.value.size;
        capacity *= 2;
        alloc = self.value.alloc as usize;
        let mut new_limbs = Vec::with_capacity(capacity);
        unsafe { new_limbs.set_len(capacity - alloc) };
        new_limbs.extend_from_slice(&old_limbs[..alloc]);
        self.limbs = new_limbs.into_boxed_slice();
    }

    fn push_u128(&mut self, value: u128) {
        self.reserve(2);
        let capacity = self.limbs.len();
        let alloc = self.value.alloc as usize;
        self.limbs[capacity - alloc - 2].write(value as u64);
        self.limbs[capacity - alloc - 1].write((value >> 64) as u64);
        self.value.alloc += 2;
        self.value.size += 2;
    }
}

struct Int128 {
    inner: u128,
    value: mpz_t,
}

impl Int128 {
    fn new(value: u128) -> Self {
        let size = if value == 0 {
            0
        } else if value < 1 << 64 {
            1
        } else {
            2
        };
        Self {
            inner: value,
            value: mpz_t {
                alloc: 2,
                size,
                d: NonNull::dangling(),
            },
        }
    }

    fn as_raw(&mut self) -> *mut mpz_t {
        self.value.d = NonNull::from(&mut self.inner).cast();
        &mut self.value
    }
}

fn main() -> io::Result<()> {
    const _: () = assert!(mem::size_of::<c_ulong>() == 8, "get a better OS");
    let filename = env::args_os().nth(1).unwrap();
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(filename)?;
    let mut log_file = BufWriter::new(LineWriter::new(log_file));
    let mut divisor = Int128::new(81_u128.pow(16));
    let divisor_raw = divisor.as_raw();
    let mut a = 2_i32;
    let mut b = Integer::new();
    let mut end = Int128::new(0);
    let mut last_end = 0;
    'outer: for i in 0_usize.. {
        let b_raw = b.as_raw();
        let end_raw = end.as_raw();
        unsafe { gmp::mpz_tdiv_qr(b_raw, end_raw, b_raw, divisor_raw) };
        let end = &mut end.inner;
        writeln!(&mut log_file, "{i} {a} {last_end} {end}")?;
        if i % 16 == 0 {
            println!("iter {}: a = {a}, b % 81^16 = {end}", i * 4 * 16);
        }
        for _ in 0..16 {
            let rem = *end % 81;
            *end /= 81;
            let (a_off, b_off) = TABLE.0[rem as usize];
            a += a_off;
            if a <= 1 {
                break 'outer;
            }
            *end = (*end << 8) + b_off as u128;
        }
        b.push_u128(*end);
        last_end = *end;
    }
    Ok(())
}
