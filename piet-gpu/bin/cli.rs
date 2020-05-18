use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use piet_gpu_hal::vulkan::VkInstance;
use piet_gpu_hal::{CmdBuf, Device, Error, MemFlags};

use piet_gpu::{render_scene, PietGpuRenderContext, Renderer, HEIGHT, WIDTH};

#[allow(unused)]
fn dump_scene(buf: &[u8]) {
    for i in 0..(buf.len() / 4) {
        let mut buf_u32 = [0u8; 4];
        buf_u32.copy_from_slice(&buf[i * 4..i * 4 + 4]);
        println!("{:4x}: {:8x}", i * 4, u32::from_le_bytes(buf_u32));
    }
}

#[allow(unused)]
fn dump_state(buf: &[u8]) {
    for i in 0..(buf.len() / 48) {
        let j = i * 48;
        let floats = (0..11).map(|k| {
            let mut buf_f32 = [0u8; 4];
            buf_f32.copy_from_slice(&buf[j + k * 4..j + k * 4 + 4]);
            f32::from_le_bytes(buf_f32)
        }).collect::<Vec<_>>();
        println!("{}: [{} {} {} {} {} {}] ({}, {})-({} {}) {} {}",
            i,
            floats[0], floats[1], floats[2], floats[3], floats[4], floats[5],
            floats[6], floats[7], floats[8], floats[9],
            floats[10], buf[j + 44]);
    }

}

/// Interpret the output of the binning stage, for diagnostic purposes.
#[allow(unused)]
fn trace_merge(buf: &[u32]) {
    for bin in 0..256 {
        println!("bin {}:", bin);
        let mut starts = (0..16).map(|i| Some((bin * 16 + i) * 64)).collect::<Vec<Option<usize>>>();
        loop {
            let min_start = starts.iter().map(|st|
                st.map(|st|
                    if buf[st / 4] == 0 {
                        !0
                    } else {
                        buf[st / 4 + 2]
                    }).unwrap_or(!0)).min().unwrap();
            if min_start == !0 {
                break;
            }
            let mut selected = !0;
            for i in 0..16 {
                if let Some(st) = starts[i] {
                    if buf[st/4] != 0 && buf[st/4 + 2] == min_start {
                        selected = i;
                        break;
                    }
                }
            }
            let st = starts[selected].unwrap();
            println!("selected {}, start {:x}", selected, st);
            for j in 0..buf[st/4] {
                println!("{:x}", buf[st/4 + 2 + j as usize])
            }
            if buf[st/4 + 1] == 0 {
                starts[selected] = None;
            } else {
                starts[selected] = Some(buf[st/4 + 1] as usize);
            }
        }

    }
}

fn analyze_log(buf: &[u32]) {
    const PER_THREAD_LOG: usize = 0x4000;
    for thread in 0..256 {
        for i in 0..PER_THREAD_LOG/2 {
            let tag = buf[thread * PER_THREAD_LOG + i * 2];
            let string = match tag {
                1 => "start thread",
                2 => "shared minimum element",
                3 => "minimum element of this thread",
                4 => "thread won",
                5 => "chosen chunk_n",
                6 => "rd_ix",
                7 => "wr_ix",
                _ => break,
            };
            println!("{}: {:x}", string, buf[thread * PER_THREAD_LOG + i * 2 + 1]);
        }
        println!("");
    }
}

fn analyze_anno(buf: &[u32]) {
    let mut count = 0;
    let mut runs = 0;
    let mut ntiles = 0.0;
    let tile_size = 16.0;
    for i in (0..13239*11).step_by(11) {
        println!("{}: {}", i, buf[i]);
        let mut last_bbox = (0., 0., 0., 0.);
        let mut bbox = (0., 0., 0., 0.);
        match buf[i] {
            1 => {
                let floats = (1..7).map(|j| f32::from_bits(buf[i + j])).collect::<Vec<_>>();
                let x0 = floats[0];
                let y0 = floats[1];
                let x1 = floats[2];
                let y1 = floats[3];
                let (x0, x1) = (x0.min(x1), x0.max(x1));
                let (y0, y1) = (y0.min(y1), y0.max(y1));
                let (x0, y0) = (x0 - floats[4], y0 - floats[5]);
                let (x1, y1) = (x1 + floats[4], y1 + floats[5]);
                println!("floats: {:?}; {} {} {} {}", floats, x0, y0, x1, y1);
                bbox = ((x0 / tile_size).floor(), (y0 / tile_size).floor(), (x1 / tile_size).ceil(), (y1 / tile_size).ceil());
            }
            4 => {
                let floats = (2..7).map(|j| f32::from_bits(buf[i + j])).collect::<Vec<_>>();
                let x0 = floats[0];
                let y0 = floats[1];
                let x1 = floats[2];
                let y1 = floats[3];
                let (x0, x1) = (x0.min(x1), x0.max(x1));
                let (y0, y1) = (y0.min(y1), y0.max(y1));
                println!("floats: {:?}; {} {} {} {}", floats, x0, y0, x1, y1);
                bbox = ((x0 / tile_size).floor(), (y0 / tile_size).floor(), (x1 / tile_size).ceil(), (y1 / tile_size).ceil());
            }
            _ => (),
        }
        let area = (bbox.2 - bbox.0) * (bbox.3 - bbox.1);
        println!("{:?}: {}", bbox, area);
        count += 1;
        if last_bbox != bbox {
            last_bbox = bbox;
            runs += 1;
        }
        ntiles += area;

    }
    println!("{} runs out of {}", runs, count);
    println!("total number of tiles: {}", ntiles);
}

fn main() -> Result<(), Error> {
    let (instance, _) = VkInstance::new(None)?;
    unsafe {
        let device = instance.device(None)?;

        let fence = device.create_fence(false)?;
        let mut cmd_buf = device.create_cmd_buf()?;
        let query_pool = device.create_query_pool(5)?;

        let mut ctx = PietGpuRenderContext::new();
        render_scene(&mut ctx);
        let scene = ctx.get_scene_buf();
        //dump_scene(&scene);

        let renderer = Renderer::new(&device, scene)?;
        let image_buf =
            device.create_buffer((WIDTH * HEIGHT * 4) as u64, MemFlags::host_coherent())?;

        cmd_buf.begin();
        renderer.record(&mut cmd_buf, &query_pool);
        cmd_buf.copy_image_to_buffer(&renderer.image_dev, &image_buf);
        cmd_buf.finish();
        device.run_cmd_buf(&cmd_buf, &[], &[], Some(&fence))?;
        device.wait_and_reset(&[fence])?;
        let ts = device.reap_query_pool(&query_pool).unwrap();
        println!("Element kernel time: {:.3}ms", ts[0] * 1e3);
        println!("Binning kernel time: {:.3}ms", (ts[1] - ts[0]) * 1e3);
        println!("Coarse kernel time: {:.3}ms", (ts[2] - ts[1]) * 1e3);
        println!("Render kernel time: {:.3}ms", (ts[3] - ts[2]) * 1e3);

        let mut data: Vec<u32> = Default::default();
        device.read_buffer(&renderer.bin_buf, &mut data).unwrap();
        //piet_gpu::dump_k1_data(&data);
        //analyze_anno(&data);
        //trace_merge(&data);

        let mut data: Vec<u32> = Default::default();
        device.read_buffer(&renderer.ptcl_buf, &mut data).unwrap();
        analyze_log(&data);

        let mut img_data: Vec<u8> = Default::default();
        // Note: because png can use a `&[u8]` slice, we could avoid an extra copy
        // (probably passing a slice into a closure). But for now: keep it simple.
        device.read_buffer(&image_buf, &mut img_data).unwrap();

        // Write image as PNG file.
        let path = Path::new("image.png");
        let file = File::create(path).unwrap();
        let ref mut w = BufWriter::new(file);

        let mut encoder = png::Encoder::new(w, WIDTH as u32, HEIGHT as u32);
        encoder.set_color(png::ColorType::RGBA);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();

        writer.write_image_data(&img_data).unwrap();
    }

    Ok(())
}
