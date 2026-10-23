/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CATransform3D`.
//!
//! touchHLE's layers only support affine transforms, so 3D transforms are
//! converted to the affine transform they amount to in the x-y plane (see
//! [CATransform3D::to_affine]).

use crate::abi::{impl_GuestRet_for_large_struct, GuestArg};
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::core_graphics::cg_affine_transform::CGAffineTransform;
use crate::frameworks::core_graphics::CGFloat;
use crate::mem::SafeRead;
use crate::Environment;
use std::ops::{Add, Mul, Sub};

/// A 4×4 matrix, used like [CGAffineTransform]: points are row vectors
/// multiplied by the matrix.
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(C, packed)]
pub struct CATransform3D {
    pub m: [[CGFloat; 4]; 4],
}
unsafe impl SafeRead for CATransform3D {}
impl GuestArg for CATransform3D {
    const REG_COUNT: usize = 16;

    fn from_regs(regs: &[u32]) -> Self {
        let mut m = [[0.0; 4]; 4];
        for (i, &reg) in regs[..16].iter().enumerate() {
            m[i / 4][i % 4] = f32::from_bits(reg);
        }
        CATransform3D { m }
    }
    fn to_regs(self, regs: &mut [u32]) {
        let m = self.m;
        for (i, reg) in regs[..16].iter_mut().enumerate() {
            *reg = m[i / 4][i % 4].to_bits();
        }
    }
}
impl_GuestRet_for_large_struct!(CATransform3D);

pub const CATransform3DIdentity: CATransform3D = CATransform3D {
    m: [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ],
};

impl CATransform3D {
    pub fn from_affine(t: CGAffineTransform) -> Self {
        CATransform3D {
            m: [
                [t.a, t.b, 0.0, 0.0],
                [t.c, t.d, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [t.tx, t.ty, 0.0, 1.0],
            ],
        }
    }
    /// The part of the transform that affects the x-y plane.
    pub fn to_affine(self) -> CGAffineTransform {
        let m = self.m;
        CGAffineTransform {
            a: m[0][0],
            b: m[0][1],
            c: m[1][0],
            d: m[1][1],
            tx: m[3][0],
            ty: m[3][1],
        }
    }
    /// `self` followed by `other`.
    pub fn concat(self, other: Self) -> Self {
        let (a, b) = (self.m, other.m);
        let mut m = [[0.0; 4]; 4];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                *value = (0..4).map(|k| a[i][k] * b[k][j]).sum();
            }
        }
        CATransform3D { m }
    }
    pub fn make_translation(x: CGFloat, y: CGFloat, z: CGFloat) -> Self {
        let mut t = CATransform3DIdentity;
        t.m[3] = [x, y, z, 1.0];
        t
    }
    pub fn make_scale(x: CGFloat, y: CGFloat, z: CGFloat) -> Self {
        let mut t = CATransform3DIdentity;
        t.m[0][0] = x;
        t.m[1][1] = y;
        t.m[2][2] = z;
        t
    }
    pub fn make_rotation(angle: CGFloat, x: CGFloat, y: CGFloat, z: CGFloat) -> Self {
        let length = (x * x + y * y + z * z).sqrt();
        if length == 0.0 {
            return CATransform3DIdentity;
        }
        let (x, y, z) = (x / length, y / length, z / length);
        let (s, c) = angle.sin_cos();
        let t = 1.0 - c;
        // The transpose of the usual (column vector) rotation matrix.
        CATransform3D {
            m: [
                [t * x * x + c, t * x * y + s * z, t * x * z - s * y, 0.0],
                [t * x * y - s * z, t * y * y + c, t * y * z + s * x, 0.0],
                [t * x * z + s * y, t * y * z - s * x, t * z * z + c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

// For interpolation in animations.
impl Add for CATransform3D {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        let (a, b) = (self.m, other.m);
        CATransform3D {
            m: std::array::from_fn(|i| std::array::from_fn(|j| a[i][j] + b[i][j])),
        }
    }
}
impl Sub for CATransform3D {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        let (a, b) = (self.m, other.m);
        CATransform3D {
            m: std::array::from_fn(|i| std::array::from_fn(|j| a[i][j] - b[i][j])),
        }
    }
}
impl Mul<CGFloat> for CATransform3D {
    type Output = Self;
    fn mul(self, factor: CGFloat) -> Self {
        let a = self.m;
        CATransform3D {
            m: std::array::from_fn(|i| std::array::from_fn(|j| a[i][j] * factor)),
        }
    }
}

fn CATransform3DIsIdentity(_env: &mut Environment, t: CATransform3D) -> bool {
    t == CATransform3DIdentity
}
fn CATransform3DMakeTranslation(
    _env: &mut Environment,
    x: CGFloat,
    y: CGFloat,
    z: CGFloat,
) -> CATransform3D {
    CATransform3D::make_translation(x, y, z)
}
fn CATransform3DMakeScale(
    _env: &mut Environment,
    x: CGFloat,
    y: CGFloat,
    z: CGFloat,
) -> CATransform3D {
    CATransform3D::make_scale(x, y, z)
}
fn CATransform3DMakeRotation(
    _env: &mut Environment,
    angle: CGFloat,
    x: CGFloat,
    y: CGFloat,
    z: CGFloat,
) -> CATransform3D {
    CATransform3D::make_rotation(angle, x, y, z)
}
// These apply the new transform first, then `t`.
fn CATransform3DTranslate(
    _env: &mut Environment,
    t: CATransform3D,
    x: CGFloat,
    y: CGFloat,
    z: CGFloat,
) -> CATransform3D {
    CATransform3D::make_translation(x, y, z).concat(t)
}
fn CATransform3DScale(
    _env: &mut Environment,
    t: CATransform3D,
    x: CGFloat,
    y: CGFloat,
    z: CGFloat,
) -> CATransform3D {
    CATransform3D::make_scale(x, y, z).concat(t)
}
fn CATransform3DRotate(
    _env: &mut Environment,
    t: CATransform3D,
    angle: CGFloat,
    x: CGFloat,
    y: CGFloat,
    z: CGFloat,
) -> CATransform3D {
    CATransform3D::make_rotation(angle, x, y, z).concat(t)
}
fn CATransform3DConcat(
    _env: &mut Environment,
    a: CATransform3D,
    b: CATransform3D,
) -> CATransform3D {
    a.concat(b)
}
fn CATransform3DMakeAffineTransform(_env: &mut Environment, t: CGAffineTransform) -> CATransform3D {
    CATransform3D::from_affine(t)
}
fn CATransform3DGetAffineTransform(_env: &mut Environment, t: CATransform3D) -> CGAffineTransform {
    t.to_affine()
}

pub const CONSTANTS: ConstantExports = &[(
    "_CATransform3DIdentity",
    HostConstant::Custom(|env| {
        env.mem
            .alloc_and_write(CATransform3DIdentity)
            .cast_void()
            .cast_const()
    }),
)];

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CATransform3DIsIdentity(_)),
    export_c_func!(CATransform3DMakeTranslation(_, _, _)),
    export_c_func!(CATransform3DMakeScale(_, _, _)),
    export_c_func!(CATransform3DMakeRotation(_, _, _, _)),
    export_c_func!(CATransform3DTranslate(_, _, _, _)),
    export_c_func!(CATransform3DScale(_, _, _, _)),
    export_c_func!(CATransform3DRotate(_, _, _, _, _)),
    export_c_func!(CATransform3DConcat(_, _)),
    export_c_func!(CATransform3DMakeAffineTransform(_)),
    export_c_func!(CATransform3DGetAffineTransform(_)),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_about_z_matches_affine_rotation() {
        let angle = 0.7;
        let t = CATransform3D::make_rotation(angle, 0.0, 0.0, 1.0).to_affine();
        let expected = CGAffineTransform::make_rotation(angle);
        assert!((t.a - expected.a).abs() < 1e-6 && (t.b - expected.b).abs() < 1e-6);
        assert!((t.c - expected.c).abs() < 1e-6 && (t.d - expected.d).abs() < 1e-6);
    }

    #[test]
    fn scale_then_translate() {
        let t = CATransform3D::make_scale(2.0, 3.0, 1.0)
            .concat(CATransform3D::make_translation(5.0, 7.0, 0.0))
            .to_affine();
        let expected = CGAffineTransform::make_scale(2.0, 3.0)
            .concat(CGAffineTransform::make_translation(5.0, 7.0));
        assert_eq!(t, expected);
    }
}
