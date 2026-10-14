//! Alpha masks for the actual drawn pose. Native cursor events refresh the
//! window's click-through flag, including while the animation clock is stopped.

/// True when the pixel at `(x, y)` in top-left image space has non-zero alpha.
pub fn opaque_at(rgba: &[u8], width: u32, height: u32, x: i32, y: i32) -> bool {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return false;
    };
    if x >= width || y >= height {
        return false;
    }
    let index = (y as usize)
        .checked_mul(width as usize)
        .and_then(|row| row.checked_add(x as usize))
        .and_then(|pixel| pixel.checked_mul(4))
        .and_then(|byte| byte.checked_add(3));
    index
        .and_then(|index| rgba.get(index))
        .copied()
        .unwrap_or(0)
        != 0
}

/// One bit per decoded pixel, shared by every mascot using this image.
#[derive(Debug)]
pub struct AlphaMask {
    width: u32,
    height: u32,
    bits: Vec<u8>,
}

impl AlphaMask {
    pub fn from_rgba(rgba: &[u8], width: u32, height: u32) -> Self {
        let count = (width as usize)
            .checked_mul(height as usize)
            .unwrap_or(0)
            .min(rgba.len() / 4);
        let mut bits = vec![0; count.div_ceil(8)];
        for (index, pixel) in rgba.chunks_exact(4).take(count).enumerate() {
            if pixel[3] != 0 {
                bits[index / 8] |= 1 << (index % 8);
            }
        }
        Self {
            width,
            height,
            bits,
        }
    }

    pub fn has_pixels(&self) -> bool {
        self.bits.iter().any(|byte| *byte != 0)
    }

    /// CGImage contents are stretched to the pose cell, including a differently
    /// sized explicit facing image.
    pub fn hit_in_cell(
        &self,
        point: (f64, f64),
        cell: (i32, i32),
        scale: f64,
        mirrored: bool,
    ) -> bool {
        if cell.0 <= 0 || cell.1 <= 0 {
            return false;
        }
        self.hit(
            point.0 * f64::from(self.width) / f64::from(cell.0),
            point.1 * f64::from(self.height) / f64::from(cell.1),
            scale,
            mirrored,
        )
    }

    /// Point relative to the unscaled sprite's top-left. CALayer scales and
    /// mirrors around the image centre; missing data and invalid input miss.
    pub fn hit(&self, x: f64, y: f64, scale: f64, mirrored: bool) -> bool {
        if !x.is_finite() || !y.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return false;
        }
        let width = f64::from(self.width);
        let height = f64::from(self.height);
        let x = (x - width / 2.0) / scale + width / 2.0;
        let y = (y - height / 2.0) / scale + height / 2.0;
        if x < 0.0 || x >= width || y < 0.0 || y >= height {
            return false;
        }
        let mut column = x.floor() as usize;
        if mirrored {
            column = self.width as usize - 1 - column;
        }
        let index = (y.floor() as usize)
            .checked_mul(self.width as usize)
            .and_then(|row| row.checked_add(column));
        index.is_some_and(|index| {
            self.bits
                .get(index / 8)
                .is_some_and(|byte| byte & (1 << (index % 8)) != 0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::opaque_at;
    #[test]
    fn opaque_centre_and_clear_corner() {
        let mut rgba = vec![0u8; 4 * 4 * 4];
        rgba[16 + 4 + 3] = 255;
        assert!(opaque_at(&rgba, 4, 4, 1, 1));
        assert!(!opaque_at(&rgba, 4, 4, 0, 0));
        assert!(!opaque_at(&rgba, 4, 4, -1, 0));
    }
}
