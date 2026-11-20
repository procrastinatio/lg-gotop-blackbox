use anyhow::Result;

/// Encode a tightly packed RGBA buffer as PNG.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header()?.write_image_data(rgba)?;
    }
    Ok(out)
}

/// Capture the whole virtual desktop (all monitors) using plain GDI.
///
/// GDI is used deliberately: DXGI Desktop Duplication is unreliable inside RDP/Citrix/VDI sessions.
#[cfg(windows)]
pub fn capture_png() -> Result<Vec<u8>> {
    use anyhow::{bail, Context};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe {
        // Without this, scaled displays yield a cropped capture.
        let _ = SetProcessDPIAware();

        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if w <= 0 || h <= 0 {
            bail!("no screen area reported (w={w}, h={h})");
        }

        let screen = GetDC(None);
        if screen.is_invalid() {
            bail!("GetDC failed");
        }
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp);

        let blt = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY | CAPTUREBLT);

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
        let lines = GetDIBits(
            mem,
            bmp,
            0,
            h as u32,
            Some(buf.as_mut_ptr().cast()),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(mem, old);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);

        blt.context("BitBlt failed")?;
        if lines == 0 {
            bail!("GetDIBits returned no scan lines");
        }

        // BGRA -> RGBA, force opaque.
        #[allow(clippy::chunks_exact_to_as_chunks)]
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }
        encode_png(w as u32, h as u32, &buf)
    }
}

#[cfg(not(windows))]
pub fn capture_png() -> Result<Vec<u8>> {
    anyhow::bail!("screenshot is only implemented on Windows")
}

#[cfg(test)]
mod tests {
    #[test]
    fn encodes_png_signature() {
        let png = super::encode_png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
    }
}
