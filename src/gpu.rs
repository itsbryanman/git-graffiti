use anyhow::Result;

#[cfg(not(feature = "opencl"))]
use anyhow::bail;

use crate::{cpu::Hit, object::PreparedCommit, sha1mid::PrefixMask};

#[cfg(feature = "opencl")]
pub fn check_available() -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "opencl"))]
pub fn check_available() -> Result<()> {
    bail!("--gpu needs the opencl feature. reinstall with --features opencl")
}

#[cfg(feature = "opencl")]
pub fn search(prepared: &PreparedCommit, mask: &PrefixMask) -> Result<Hit> {
    use anyhow::Context;
    use ocl::{Buffer, ProQue, flags};

    const CHUNK: usize = 1 << 20;
    let pro_que = ProQue::builder()
        .src(include_str!("../kernels/sha1.cl"))
        .dims(CHUNK)
        .build()
        .context("could not start OpenCL")?;
    let queue = pro_que.queue().clone();
    let midstate = Buffer::<u32>::builder()
        .queue(queue.clone())
        .flags(flags::MEM_READ_ONLY)
        .len(5)
        .copy_host_slice(&prepared.midstate)
        .build()?;
    let target = Buffer::<u8>::builder()
        .queue(queue.clone())
        .flags(flags::MEM_READ_ONLY)
        .len(20)
        .copy_host_slice(mask.bytes())
        .build()?;
    let found = Buffer::<i32>::builder()
        .queue(queue.clone())
        .len(1)
        .fill_val(0_i32)
        .build()?;
    let found_nonce = Buffer::<u64>::builder()
        .queue(queue)
        .len(1)
        .fill_val(0_u64)
        .build()?;
    let kernel = pro_que
        .kernel_builder("mine")
        .arg(&midstate)
        .arg(&target)
        .arg(mask.nibbles() as u32)
        .arg(prepared.input_bit_length)
        .arg(0_u64)
        .arg(&found)
        .arg(&found_nonce)
        .build()?;

    let mut start = 0_u64;
    loop {
        kernel.set_arg(4, start)?;
        // The buffers outlive the command, and this call waits before changing an arg.
        unsafe {
            kernel.cmd().global_work_size(CHUNK).enq()?;
        }
        let mut did_find = [0_i32; 1];
        found.read(&mut did_find[..]).enq()?;
        if did_find[0] != 0 {
            let mut nonce = [0_u64; 1];
            found_nonce.read(&mut nonce[..]).enq()?;
            return Ok(Hit {
                nonce: nonce[0],
                digest: prepared.digest_for_nonce(nonce[0]),
            });
        }
        start = start
            .checked_add(CHUNK as u64)
            .context("searched the entire nonce space without a hit")?;
    }
}

#[cfg(not(feature = "opencl"))]
pub fn search(_prepared: &PreparedCommit, _mask: &PrefixMask) -> Result<Hit> {
    check_available()?;
    unreachable!()
}
