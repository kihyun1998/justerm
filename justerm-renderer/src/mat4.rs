//! A 4×4 column-major matrix — just enough for the orthographic projection the
//! vertex shader needs. Pure, host-testable (the GL upload of `data` is browser-only).
//!
//! The renderer works in pixel space (origin top-left, +y down) and projects to
//! WebGL's NDC (`-1..1`, +y up) with an off-centre orthographic matrix, mirroring
//! beamterm's `mat4::orthographic_from_size`.

/// Column-major 4×4, laid out for direct upload as a `mat4` uniform.
#[derive(Debug, Clone, PartialEq)]
pub struct Mat4 {
    pub data: [f32; 16],
}

impl Mat4 {
    /// Identity.
    pub fn identity() -> Self {
        let mut data = [0.0; 16];
        data[0] = 1.0;
        data[5] = 1.0;
        data[10] = 1.0;
        data[15] = 1.0;
        Self { data }
    }

    /// Orthographic projection for a `width`×`height` pixel viewport with the origin
    /// at the top-left (+y down): pixel `(0,0)` → NDC `(-1, 1)`, `(width,height)` →
    /// NDC `(1, -1)`. Equivalent to `ortho(left=0, right=width, bottom=height, top=0,
    /// near=-1, far=1)`.
    pub fn orthographic_from_size(width: f32, height: f32) -> Self {
        // ortho(left=0, right=width, bottom=height, top=0, near=-1, far=1).
        let (left, right, bottom, top, near, far) = (0.0, width, height, 0.0, -1.0, 1.0);
        let mut m = Self::identity();
        let d = &mut m.data;
        d[0] = 2.0 / (right - left);
        d[5] = 2.0 / (top - bottom);
        d[10] = -2.0 / (far - near);
        d[12] = -(right + left) / (right - left);
        d[13] = -(top + bottom) / (top - bottom);
        d[14] = -(far + near) / (far - near);
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Column-major mat4 · (x, y, 0, 1). Only the entries the ortho fills matter.
    fn project(m: &Mat4, x: f32, y: f32) -> [f32; 2] {
        let d = &m.data;
        [d[0] * x + d[4] * y + d[12], d[1] * x + d[5] * y + d[13]]
    }

    #[test]
    fn maps_pixel_corners_to_ndc_top_left_origin() {
        // 320×200 viewport. Corners worked by hand from the top-left-origin ortho
        // definition (NOT recomputed the way the code builds the matrix):
        //   (0,0)       -> (-1,  1)   top-left
        //   (320,200)   -> ( 1, -1)   bottom-right
        //   (160,100)   -> ( 0,  0)   centre
        let m = Mat4::orthographic_from_size(320.0, 200.0);

        let tl = project(&m, 0.0, 0.0);
        let br = project(&m, 320.0, 200.0);
        let c = project(&m, 160.0, 100.0);

        assert!(
            (tl[0] + 1.0).abs() < 1e-6 && (tl[1] - 1.0).abs() < 1e-6,
            "tl={tl:?}"
        );
        assert!(
            (br[0] - 1.0).abs() < 1e-6 && (br[1] + 1.0).abs() < 1e-6,
            "br={br:?}"
        );
        assert!(c[0].abs() < 1e-6 && c[1].abs() < 1e-6, "c={c:?}");
    }
}
