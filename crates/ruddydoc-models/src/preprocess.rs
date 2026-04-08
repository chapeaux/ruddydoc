//! Image preprocessing utilities for ML model input.
//!
//! All operations are implemented in pure Rust without external image
//! processing libraries, keeping this crate lightweight.

/// Standard ImageNet normalization mean values (per channel, RGB).
pub const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];

/// Standard ImageNet normalization standard deviation values (per channel, RGB).
pub const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// Convert HWC (height, width, channels) u8 data to CHW (channels, height, width)
/// f32 data, normalizing pixel values from `[0, 255]` to `[0.0, 1.0]`.
///
/// # Arguments
/// * `data` - Raw pixel data in HWC layout, u8 values.
/// * `width` - Image width in pixels.
/// * `height` - Image height in pixels.
/// * `channels` - Number of channels (typically 3 for RGB).
pub fn hwc_to_chw(data: &[u8], width: u32, height: u32, channels: u32) -> Vec<f32> {
    let w = width as usize;
    let h = height as usize;
    let c = channels as usize;
    let pixel_count = w * h;
    let mut chw = vec![0.0f32; c * pixel_count];

    for y in 0..h {
        for x in 0..w {
            let hwc_idx = (y * w + x) * c;
            for ch in 0..c {
                let chw_idx = ch * pixel_count + y * w + x;
                chw[chw_idx] = data[hwc_idx + ch] as f32 / 255.0;
            }
        }
    }

    chw
}

/// Normalize pixel values in-place: `(pixel - mean) / std` per channel.
///
/// The data must be in CHW layout with `channels` channels. Each channel
/// occupies a contiguous block of `data.len() / channels` elements.
///
/// # Arguments
/// * `data` - Pixel data in CHW layout, modified in-place.
/// * `channels` - Number of channels (must be <= 3).
/// * `mean` - Per-channel mean values.
/// * `std` - Per-channel standard deviation values.
pub fn normalize(data: &mut [f32], channels: u32, mean: [f32; 3], std: [f32; 3]) {
    let c = channels as usize;
    if c == 0 {
        return;
    }
    let pixels_per_channel = data.len() / c;

    for ch in 0..c {
        let start = ch * pixels_per_channel;
        let end = start + pixels_per_channel;
        let m = mean[ch];
        let s = std[ch];
        for pixel in &mut data[start..end] {
            *pixel = (*pixel - m) / s;
        }
    }
}

/// Resize image data using bilinear interpolation.
///
/// The data must be in CHW layout. The function operates on each channel
/// independently.
///
/// # Arguments
/// * `data` - Source pixel data in CHW layout.
/// * `src_w` - Source width.
/// * `src_h` - Source height.
/// * `dst_w` - Target width.
/// * `dst_h` - Target height.
/// * `channels` - Number of channels.
pub fn resize_bilinear(
    data: &[f32],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    channels: u32,
) -> Vec<f32> {
    let sw = src_w as usize;
    let sh = src_h as usize;
    let dw = dst_w as usize;
    let dh = dst_h as usize;
    let c = channels as usize;
    let src_pixels = sw * sh;
    let dst_pixels = dw * dh;

    let mut result = vec![0.0f32; c * dst_pixels];

    // Scale factors: map destination coordinates to source coordinates.
    let x_scale = if dw > 1 {
        (sw as f64 - 1.0) / (dw as f64 - 1.0)
    } else {
        0.0
    };
    let y_scale = if dh > 1 {
        (sh as f64 - 1.0) / (dh as f64 - 1.0)
    } else {
        0.0
    };

    for ch in 0..c {
        let src_offset = ch * src_pixels;
        let dst_offset = ch * dst_pixels;

        for dy in 0..dh {
            let src_y = dy as f64 * y_scale;
            let y0 = src_y.floor() as usize;
            let y1 = y0.saturating_add(1).min(sh - 1);
            let fy = (src_y - y0 as f64) as f32;

            for dx in 0..dw {
                let src_x = dx as f64 * x_scale;
                let x0 = src_x.floor() as usize;
                let x1 = x0.saturating_add(1).min(sw - 1);
                let fx = (src_x - x0 as f64) as f32;

                // Bilinear interpolation of the four surrounding pixels.
                let p00 = data[src_offset + y0 * sw + x0];
                let p10 = data[src_offset + y0 * sw + x1];
                let p01 = data[src_offset + y1 * sw + x0];
                let p11 = data[src_offset + y1 * sw + x1];

                let top = p00 * (1.0 - fx) + p10 * fx;
                let bottom = p01 * (1.0 - fx) + p11 * fx;
                let value = top * (1.0 - fy) + bottom * fy;

                result[dst_offset + dy * dw + dx] = value;
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hwc_to_chw_basic() {
        // 2x1 image, RGB: pixel0=(255,0,0), pixel1=(0,255,0)
        let hwc: Vec<u8> = vec![255, 0, 0, 0, 255, 0];
        let chw = hwc_to_chw(&hwc, 2, 1, 3);

        assert_eq!(chw.len(), 6);
        // Channel 0 (R): [1.0, 0.0]
        assert!((chw[0] - 1.0).abs() < 1e-6);
        assert!((chw[1]).abs() < 1e-6);
        // Channel 1 (G): [0.0, 1.0]
        assert!((chw[2]).abs() < 1e-6);
        assert!((chw[3] - 1.0).abs() < 1e-6);
        // Channel 2 (B): [0.0, 0.0]
        assert!((chw[4]).abs() < 1e-6);
        assert!((chw[5]).abs() < 1e-6);
    }

    #[test]
    fn hwc_to_chw_2x2() {
        // 2x2 all-white image
        let hwc: Vec<u8> = vec![255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255];
        let chw = hwc_to_chw(&hwc, 2, 2, 3);

        assert_eq!(chw.len(), 12);
        for val in &chw {
            assert!((val - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn normalize_imagenet() {
        // Single pixel per channel, value = 0.5
        let mut data = vec![0.5, 0.5, 0.5];
        normalize(&mut data, 3, IMAGENET_MEAN, IMAGENET_STD);

        // channel 0: (0.5 - 0.485) / 0.229
        let expected_0 = (0.5 - 0.485) / 0.229;
        assert!((data[0] - expected_0).abs() < 1e-5);
        // channel 1: (0.5 - 0.456) / 0.224
        let expected_1 = (0.5 - 0.456) / 0.224;
        assert!((data[1] - expected_1).abs() < 1e-5);
        // channel 2: (0.5 - 0.406) / 0.225
        let expected_2 = (0.5 - 0.406) / 0.225;
        assert!((data[2] - expected_2).abs() < 1e-5);
    }

    #[test]
    fn normalize_preserves_length() {
        let mut data = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        normalize(&mut data, 3, IMAGENET_MEAN, IMAGENET_STD);
        assert_eq!(data.len(), 6);
    }

    #[test]
    fn normalize_zero_channels() {
        let mut data = vec![0.5];
        normalize(&mut data, 0, IMAGENET_MEAN, IMAGENET_STD);
        // Should not modify anything
        assert!((data[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn resize_bilinear_identity() {
        // 2x2 single-channel image, resize to same size
        let data = vec![0.0, 1.0, 0.0, 1.0];
        let resized = resize_bilinear(&data, 2, 2, 2, 2, 1);
        assert_eq!(resized.len(), 4);
        for (a, b) in data.iter().zip(resized.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn resize_bilinear_upscale() {
        // 2x2 single-channel: [[0, 1], [0, 1]] -> 4x4
        let data = vec![0.0, 1.0, 0.0, 1.0];
        let resized = resize_bilinear(&data, 2, 2, 4, 4, 1);
        assert_eq!(resized.len(), 16);

        // Corners should match original corners
        assert!((resized[0]).abs() < 1e-6); // top-left
        assert!((resized[3] - 1.0).abs() < 1e-6); // top-right
        assert!((resized[12]).abs() < 1e-6); // bottom-left
        assert!((resized[15] - 1.0).abs() < 1e-6); // bottom-right

        // Center values should be interpolated
        // At (1, 1) in 4x4: src_x = 1/3 * 1 = 0.333, src_y = 1/3 * 1 = 0.333
        // p00=0, p10=1, p01=0, p11=1 => top=0.333, bottom=0.333 => 0.333
        let center = resized[1 * 4 + 1];
        assert!(center > 0.0 && center < 1.0);
    }

    #[test]
    fn resize_bilinear_downscale() {
        // 4x4 single-channel all 0.5 -> 2x2
        let data = vec![0.5; 16];
        let resized = resize_bilinear(&data, 4, 4, 2, 2, 1);
        assert_eq!(resized.len(), 4);
        for val in &resized {
            assert!((val - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn resize_bilinear_multichannel() {
        // 2x2 RGB image, resize to 3x3
        // Channel R: [[1, 0], [0, 1]]
        // Channel G: [[0, 1], [1, 0]]
        // Channel B: [[0.5, 0.5], [0.5, 0.5]]
        let data = vec![
            1.0, 0.0, 0.0, 1.0, // R
            0.0, 1.0, 1.0, 0.0, // G
            0.5, 0.5, 0.5, 0.5, // B
        ];
        let resized = resize_bilinear(&data, 2, 2, 3, 3, 3);
        assert_eq!(resized.len(), 27); // 3 channels * 3 * 3

        // B channel should be all 0.5
        for i in 18..27 {
            assert!((resized[i] - 0.5).abs() < 1e-5);
        }
    }

    #[test]
    fn resize_bilinear_1x1() {
        // 1x1 -> anything should replicate
        let data = vec![0.7];
        let resized = resize_bilinear(&data, 1, 1, 3, 3, 1);
        assert_eq!(resized.len(), 9);
        for val in &resized {
            assert!((val - 0.7).abs() < 1e-6);
        }
    }
}
