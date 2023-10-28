use rayon::prelude::*;
use std::{
    arch::asm,
    collections::VecDeque,
    env,
    fs::{File, OpenOptions},
    io::{self, prelude::*, BufWriter, LineWriter},
    slice,
    time::{Duration, Instant},
};

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
    (-2, 237), (1, 239), (1, 242), (2, 245), (2, 248), (0, 252), (1, 255)
]);

// adapted from https://gmplib.org/~tege/division-paper.pdf
fn multiply_limbs_256(limbs: &mut [u128], mut carry: u128) -> u128 {
    const D: u128 = 81_u128.pow(16) << 26;
    const D1: u64 = (D >> 64) as u64;
    const D0: u64 = D as u64;
    const V: u64 = 0x7a0a91194ed93d67;
    let i2p = |x: u128| ((x >> 64) as u64, x as u64);
    let p2i = |x: u64, y: u64| (x as u128) << 64 | y as u128;
    let div_3by2 = |u21: u128, u0: u64| -> (u128, u64) {
        let (u2, u1) = i2p(u21);
        let q = u2 as u128 * V as u128;
        let (mut q1, q0) = i2p(q + u21);
        let r1 = u1 - q1 * D1;
        let t = q1 as u128 * D0 as u128;
        let mut r = p2i(r1, u0) - t - D;
        q1 += 1;
        if i2p(r).0 >= q0 {
            q1 -= 1;
            r += D;
        }
        if i2p(r).0 >= D1 {
            // generate a real branch instead of conditional moves
            unsafe { asm!("", options(nomem, preserves_flags, nostack)) };
            if r >= D {
                q1 += 1;
                r -= D;
            }
        }
        (r, q1)
    };
    for limb in limbs {
        let r = *limb << 26 | carry >> 102;
        let (u0a, u0b) = i2p(carry << 26);
        let (r, q1a) = div_3by2(r, u0a);
        let (r, q1b) = div_3by2(r, u0b);
        *limb = r >> 26;
        carry = p2i(q1a, q1b);
    }
    carry
}

type Page = Align4096<[u128; 256]>;

struct Integer {
    pages: VecDeque<Page>,
    start: usize,
}

impl Integer {
    fn new() -> Self {
        Self {
            pages: [Align4096([0; 256])].into(),
            start: 0,
        }
    }

    fn pop(&mut self) -> u128 {
        let limb = self.pages[0].0[self.start];
        self.start += 1;
        if self.start >= 256 {
            self.start = 0;
            self.pages.pop_front().unwrap();
        }
        limb
    }

    fn add(&mut self, i: usize, mut j: usize, mut value: u128) {
        if value == 0 {
            return;
        }
        for page in self.pages.iter_mut().skip(i) {
            while j < page.0.len() {
                let (mut sum, ov) = page.0[j].overflowing_add(value);
                if ov {
                    sum -= 81_u128.pow(16);
                }
                page.0[j] = sum % 81_u128.pow(16);
                value = sum / 81_u128.pow(16) + ov as u128;
                if value == 0 {
                    return;
                }
                j += 1;
            }
            j = 0;
        }
        let mut page = Align4096([0; 256]);
        page.0[0] = value % 81_u128.pow(16);
        page.0[1] = value / 81_u128.pow(16);
        self.pages.push_back(page);
    }

    fn multiply_256(&mut self, carries: &mut Vec<(usize, u128)>) {
        let len = self.pages.len();
        let chunk_size = len.div_ceil(rayon::current_num_threads() * 16);
        let start = self.start;
        self.pages
            .par_iter_mut()
            .enumerate()
            .fold_chunks_with(chunk_size, (usize::MAX, 0), move |(_, carry), (i, page)| {
                let start = if i == 0 { start } else { 0 };
                let carry = multiply_limbs_256(&mut page.0[start..], carry);
                (i + 1, carry)
            })
            .collect_into_vec(carries);
        for &(i, carry) in &carries[..] {
            self.add(i, 0, carry);
        }
    }

    fn push(&mut self, limb: u128, carries: &mut Vec<(usize, u128)>) {
        self.multiply_256(carries);
        self.add(0, self.start, limb);
    }
}

fn save(i: u64, a: i64, b: &Integer, save_file: &mut Option<File>) -> io::Result<()> {
    let Some(save_file) = save_file else {
        return Ok(());
    };
    save_file.rewind()?;
    save_file.write_all(bytemuck::bytes_of(&i))?;
    save_file.write_all(bytemuck::bytes_of(&a))?;
    let b_len = b.pages.len() * 4096 - b.start * 16;
    save_file.write_all(bytemuck::bytes_of(&b_len))?;
    let slices = b.pages.as_slices();
    let slice = unsafe { slice::from_raw_parts(slices.0.as_ptr().cast(), slices.0.len() * 4096) };
    save_file.write_all(&slice[b.start * 16..])?;
    let slice = unsafe { slice::from_raw_parts(slices.1.as_ptr().cast(), slices.1.len() * 4096) };
    save_file.write_all(slice)?;
    if let Err(err) = save_file.sync_data() {
        if err.kind() != io::ErrorKind::InvalidInput {
            return Err(err);
        }
    }
    Ok(())
}

fn restore(i: &mut u64, a: &mut i64, b: &mut Integer, mut restore_file: File) -> io::Result<()> {
    restore_file.read_exact(bytemuck::bytes_of_mut(i))?;
    restore_file.read_exact(bytemuck::bytes_of_mut(a))?;
    let mut b_len = 0_usize;
    restore_file.read_exact(bytemuck::bytes_of_mut(&mut b_len))?;
    let mut pages = Vec::new();
    pages.resize_with(b_len.div_ceil(4096), || Align4096([0; 256]));
    let slice = unsafe { slice::from_raw_parts_mut(pages.as_mut_ptr().cast(), b_len) };
    restore_file.read_exact(slice)?;
    b.pages = pages.into();
    b.start = 0;
    Ok(())
}

fn main() -> io::Result<()> {
    let mut args = env::args_os().fuse().skip(1);
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(args.next().unwrap())?;
    let mut log_file = BufWriter::new(LineWriter::new(log_file));
    let mut i = 0_u64;
    let mut a = 2_i64;
    let mut b = Integer::new();
    let save_filename = args.next();
    if let Some(restore_file) = args.next().map(File::open) {
        restore(&mut i, &mut a, &mut b, restore_file?)?;
    }
    let mut save_file = save_filename
        .map(|filename| OpenOptions::new().create(true).write(true).open(filename))
        .transpose()?;
    let mut carries = Vec::new();
    let mut last_end = 0;
    let now = Instant::now();
    let mut next_save = now + Duration::from_secs(300);
    let mut next_status = now + Duration::from_secs(1);
    loop {
        let now = Instant::now();
        if now > next_save {
            next_save = now + Duration::from_secs(300);
            if save(i, a, &b, &mut save_file).is_err() {
                eprintln!("warning: could not save state to file");
            }
        }
        let mut end = b.pop();
        writeln!(&mut log_file, "{i} {a} {last_end} {end}")?;
        if now > next_status {
            next_status = now + Duration::from_secs(1);
            println!("iter {}: a = {a}, b % 81^16 = {end}", i * 4 * 16);
        }
        for _ in 0..16 {
            let rem = end % 81;
            end /= 81;
            let (a_off, b_off) = TABLE.0[rem as usize];
            a += a_off as i64;
            if a <= 1 {
                return Ok(());
            }
            end = (end << 8) + b_off as u128;
        }
        b.push(end, &mut carries);
        last_end = end;
        i += 1;
    }
}
