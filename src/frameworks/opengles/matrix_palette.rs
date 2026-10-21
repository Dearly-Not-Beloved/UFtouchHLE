/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Emulation of the `OES_matrix_palette` extension (vertex skinning).
//!
//! The iPhone supports this extension, but neither the GLES1-on-GL2 layer nor
//! every Android driver does, so it is emulated for all backends: the palette
//! state is kept here, and draws with the palette enabled are skinned on the
//! CPU. Each vertex is transformed to eye space by its weighted palette
//! matrices, then drawn with an identity modelview matrix, which is the same as
//! what the extension does in the vertex pipeline.
//!
//! Vertex data can be in buffer objects, which can't be read back with
//! OpenGL ES, so a copy of each buffer's data is kept too.

use crate::gles::gles11_raw as gles11; // constants only
use crate::gles::gles11_raw::types::{GLenum, GLfloat, GLint, GLsizei, GLuint, GLvoid};
use crate::gles::GLES;
use crate::mem::{ConstVoidPtr, Mem, Ptr};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Same as the PowerVR SGX in later iPhones.
pub const MAX_PALETTE_MATRICES: usize = 11;
/// Same as the PowerVR SGX in later iPhones.
pub const MAX_VERTEX_UNITS: usize = 4;

/// Column-major 4×4 matrix, like OpenGL's.
type Matrix = [f32; 16];

const IDENTITY: Matrix = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, //
];

/// The parameters of a `gl*Pointer` call.
#[derive(Clone, Copy)]
pub struct ArrayPointer {
    pub size: GLint,
    pub type_: GLenum,
    pub stride: GLsizei,
    /// A guest pointer, or an offset if `buffer` isn't 0.
    pub pointer: ConstVoidPtr,
    /// The `ARRAY_BUFFER_BINDING` when the pointer was set.
    pub buffer: GLuint,
}
impl Default for ArrayPointer {
    fn default() -> Self {
        ArrayPointer {
            size: 4,
            type_: gles11::FLOAT,
            stride: 0,
            pointer: Ptr::null(),
            buffer: 0,
        }
    }
}

/// Per-context state.
pub struct State {
    /// Whether the matrix mode is `MATRIX_PALETTE_OES`. The underlying context
    /// is left in the previous matrix mode meanwhile.
    pub palette_mode: bool,
    pub current_matrix: usize,
    matrices: [Matrix; MAX_PALETTE_MATRICES],
    /// `MATRIX_PALETTE_OES` capability
    pub enabled: bool,
    pub weight_array_enabled: bool,
    pub matrix_index_array_enabled: bool,
    pub weight_pointer: ArrayPointer,
    pub matrix_index_pointer: ArrayPointer,
    /// Tracked so the skinned data can be read and the pointers restored.
    pub vertex_pointer: ArrayPointer,
    pub normal_pointer: ArrayPointer,
    /// Copies of the data of buffer objects, shared by contexts in the same
    /// sharegroup.
    pub buffers: Rc<RefCell<HashMap<GLuint, Vec<u8>>>>,
}
impl Default for State {
    fn default() -> Self {
        State {
            palette_mode: false,
            current_matrix: 0,
            matrices: [IDENTITY; MAX_PALETTE_MATRICES],
            enabled: false,
            weight_array_enabled: false,
            matrix_index_array_enabled: false,
            weight_pointer: ArrayPointer {
                size: 0,
                ..Default::default()
            },
            matrix_index_pointer: ArrayPointer {
                size: 0,
                type_: gles11::UNSIGNED_BYTE,
                ..Default::default()
            },
            vertex_pointer: Default::default(),
            normal_pointer: ArrayPointer {
                size: 3,
                ..Default::default()
            },
            buffers: Default::default(),
        }
    }
}

impl State {
    /// Whether draws need to be skinned.
    pub fn is_active(&self) -> bool {
        self.enabled && self.weight_array_enabled && self.matrix_index_array_enabled
    }

    pub fn current_matrix_mut(&mut self) -> &mut Matrix {
        &mut self.matrices[self.current_matrix]
    }

    pub fn load_identity(&mut self) {
        self.load_matrix(IDENTITY);
    }

    pub fn load_matrix(&mut self, matrix: Matrix) {
        *self.current_matrix_mut() = matrix;
    }

    /// Multiplies the current palette matrix by `matrix`, like `glMultMatrix`.
    pub fn mult_matrix(&mut self, matrix: &Matrix) {
        let current = self.current_matrix_mut();
        *current = multiply(current, matrix);
    }

    pub fn translate(&mut self, x: f32, y: f32, z: f32) {
        let mut matrix = IDENTITY;
        matrix[12..15].copy_from_slice(&[x, y, z]);
        self.mult_matrix(&matrix);
    }

    pub fn scale(&mut self, x: f32, y: f32, z: f32) {
        let mut matrix = IDENTITY;
        matrix[0] = x;
        matrix[5] = y;
        matrix[10] = z;
        self.mult_matrix(&matrix);
    }

    pub fn rotate(&mut self, angle_degrees: f32, x: f32, y: f32, z: f32) {
        let length = (x * x + y * y + z * z).sqrt();
        if length == 0.0 {
            return;
        }
        let (x, y, z) = (x / length, y / length, z / length);
        let (s, c) = angle_degrees.to_radians().sin_cos();
        let t = 1.0 - c;
        #[rustfmt::skip]
        let matrix = [
            x * x * t + c,     y * x * t + z * s, x * z * t - y * s, 0.0,
            x * y * t - z * s, y * y * t + c,     y * z * t + x * s, 0.0,
            x * z * t + y * s, y * z * t - x * s, z * z * t + c,     0.0,
            0.0,               0.0,               0.0,               1.0,
        ];
        self.mult_matrix(&matrix);
    }

    /// Updates the copy of a buffer's data for `glBufferData`.
    pub fn buffer_data(&mut self, buffer: GLuint, data: Option<&[u8]>, size: usize) {
        if buffer == 0 {
            return;
        }
        let data = data.map_or_else(|| vec![0; size], |data| data.to_vec());
        self.buffers.borrow_mut().insert(buffer, data);
    }

    /// Updates the copy of a buffer's data for `glBufferSubData`.
    pub fn buffer_sub_data(&mut self, buffer: GLuint, offset: usize, data: &[u8]) {
        let mut buffers = self.buffers.borrow_mut();
        let Some(copy) = buffers.get_mut(&buffer) else {
            return;
        };
        if let Some(target) = copy.get_mut(offset..offset + data.len()) {
            target.copy_from_slice(data);
        }
    }

    pub fn delete_buffer(&mut self, buffer: GLuint) {
        self.buffers.borrow_mut().remove(&buffer);
    }
}

/// `a × b`
fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut result = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            result[column * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[column * 4 + k]).sum();
        }
    }
    result
}

fn transform_point(m: &Matrix, p: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|row| (0..4).map(|k| m[k * 4 + row] * p[k]).sum())
}

/// The inverse transpose of the upper-left 3×3 part of a matrix, which is
/// what normals are transformed by, as columns.
fn normal_matrix(m: &Matrix) -> [[f32; 3]; 3] {
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let c0 = [m[0], m[1], m[2]];
    let c1 = [m[4], m[5], m[6]];
    let c2 = [m[8], m[9], m[10]];
    let (x0, x1, x2) = (cross(c1, c2), cross(c2, c0), cross(c0, c1));
    let det: f32 = (0..3).map(|i| c0[i] * x0[i]).sum();
    if det == 0.0 {
        return [[0.0; 3]; 3];
    }
    [x0, x1, x2].map(|column| column.map(|value| value / det))
}

/// From `OES_element_index_uint`, for indices.
const UNSIGNED_INT: GLenum = 0x1405;

fn type_size(type_: GLenum) -> Option<usize> {
    match type_ {
        gles11::BYTE | gles11::UNSIGNED_BYTE => Some(1),
        gles11::SHORT | gles11::UNSIGNED_SHORT => Some(2),
        gles11::FIXED | gles11::FLOAT | UNSIGNED_INT => Some(4),
        _ => None,
    }
}

/// Reads one component. `normalize` maps signed integers to [-1, 1], as for
/// normals.
fn read_component(bytes: &[u8], type_: GLenum, normalize: bool) -> f32 {
    match type_ {
        gles11::FLOAT => f32::from_le_bytes(bytes[..4].try_into().unwrap()),
        gles11::FIXED => i32::from_le_bytes(bytes[..4].try_into().unwrap()) as f32 / 65536.0,
        gles11::SHORT => {
            let value = i16::from_le_bytes(bytes[..2].try_into().unwrap()) as f32;
            if normalize {
                (2.0 * value + 1.0) / 65535.0
            } else {
                value
            }
        }
        gles11::BYTE => {
            let value = bytes[0] as i8 as f32;
            if normalize {
                (2.0 * value + 1.0) / 255.0
            } else {
                value
            }
        }
        gles11::UNSIGNED_BYTE => bytes[0] as f32,
        _ => unreachable!(),
    }
}

/// An array's data, for reading the attributes of each vertex.
struct ArrayData<'a> {
    bytes: &'a [u8],
    stride: usize,
    size: usize,
    type_: GLenum,
    type_size: usize,
}
impl ArrayData<'_> {
    fn get(&self, vertex: usize, component: usize, normalize: bool) -> f32 {
        let offset = vertex * self.stride + component * self.type_size;
        read_component(&self.bytes[offset..], self.type_, normalize)
    }
}

/// Gets the data of `vertex_count` vertices of an array, from the guest memory
/// or from the copy of a buffer object.
fn array_data<'a>(
    mem: &'a Mem,
    buffers: &'a HashMap<GLuint, Vec<u8>>,
    pointer: &ArrayPointer,
    vertex_count: usize,
) -> Option<ArrayData<'a>> {
    let type_size = type_size(pointer.type_)?;
    let size: usize = pointer.size.try_into().ok()?;
    let element_size = size * type_size;
    let stride = match pointer.stride {
        0 => element_size,
        stride => stride.try_into().ok()?,
    };
    let length = stride * (vertex_count - 1) + element_size;
    let bytes = if pointer.buffer != 0 {
        let offset = pointer.pointer.to_bits() as usize;
        buffers.get(&pointer.buffer)?.get(offset..offset + length)?
    } else {
        if pointer.pointer.is_null() {
            return None;
        }
        mem.bytes_at(pointer.pointer.cast(), length.try_into().ok()?)
    };
    Some(ArrayData {
        bytes,
        stride,
        size,
        type_: pointer.type_,
        type_size,
    })
}

/// The highest vertex index used by a `glDrawElements` call, or [None] if the
/// indices can't be read.
pub fn max_element_index(
    state: &State,
    mem: &Mem,
    element_buffer: GLuint,
    count: GLsizei,
    type_: GLenum,
    indices: ConstVoidPtr,
) -> Option<u32> {
    let count: usize = count.try_into().ok()?;
    if count == 0 {
        return None;
    }
    let pointer = ArrayPointer {
        size: 1,
        type_,
        stride: 0,
        pointer: indices,
        buffer: element_buffer,
    };
    let buffers = state.buffers.borrow();
    let data = array_data(mem, &buffers, &pointer, count)?;
    (0..count)
        .map(|i| {
            let bytes = &data.bytes[i * data.type_size..];
            match type_ {
                gles11::UNSIGNED_BYTE => bytes[0].into(),
                gles11::UNSIGNED_SHORT => u16::from_le_bytes(bytes[..2].try_into().unwrap()).into(),
                _ => u32::from_le_bytes(bytes[..4].try_into().unwrap()),
            }
        })
        .max()
}

/// Translates a tracked pointer to what the underlying context expects.
fn host_pointer(mem: &Mem, pointer: &ArrayPointer) -> *const GLvoid {
    if pointer.buffer != 0 {
        pointer.pointer.to_bits() as usize as *const GLvoid
    } else if pointer.pointer.is_null() {
        std::ptr::null()
    } else {
        mem.unchecked_ptr_at(pointer.pointer.cast::<u8>(), 0).cast()
    }
}

/// Skinned eye-space positions (4 components) and normals (3 components) of
/// vertices `0..vertex_count`.
fn skin(
    state: &State,
    mem: &Mem,
    vertex_count: usize,
    with_normals: bool,
) -> Option<(Vec<GLfloat>, Vec<GLfloat>)> {
    let buffers = state.buffers.borrow();
    let positions = array_data(mem, &buffers, &state.vertex_pointer, vertex_count)?;
    let weights = array_data(mem, &buffers, &state.weight_pointer, vertex_count)?;
    let indices = array_data(mem, &buffers, &state.matrix_index_pointer, vertex_count)?;
    let normals = if with_normals {
        Some(array_data(
            mem,
            &buffers,
            &state.normal_pointer,
            vertex_count,
        )?)
    } else {
        None
    };
    let units = weights.size.min(indices.size).min(MAX_VERTEX_UNITS);
    let normal_matrices = state.matrices.map(|matrix| normal_matrix(&matrix));

    let mut out_positions = Vec::with_capacity(vertex_count * 4);
    let mut out_normals = Vec::with_capacity(if with_normals { vertex_count * 3 } else { 0 });
    for vertex in 0..vertex_count {
        let mut position = [0.0, 0.0, 0.0, 1.0];
        for (i, value) in position.iter_mut().enumerate().take(positions.size) {
            *value = positions.get(vertex, i, false);
        }
        let normal: [f32; 3] = match normals {
            Some(ref normals) => std::array::from_fn(|i| normals.get(vertex, i, true)),
            None => [0.0; 3],
        };

        let mut skinned_position = [0.0; 4];
        let mut skinned_normal = [0.0; 3];
        for unit in 0..units {
            let weight = weights.get(vertex, unit, false);
            let index = indices.get(vertex, unit, false) as usize;
            if weight == 0.0 || index >= MAX_PALETTE_MATRICES {
                continue;
            }
            let transformed = transform_point(&state.matrices[index], position);
            for i in 0..4 {
                skinned_position[i] += weight * transformed[i];
            }
            if with_normals {
                let columns = &normal_matrices[index];
                for i in 0..3 {
                    skinned_normal[i] +=
                        weight * (0..3).map(|k| columns[k][i] * normal[k]).sum::<f32>();
                }
            }
        }
        out_positions.extend_from_slice(&skinned_position);
        if with_normals {
            out_normals.extend_from_slice(&skinned_normal);
        }
    }
    Some((out_positions, out_normals))
}

/// Does a draw call (`draw`) using vertices `0..vertex_count` with skinning.
///
/// # Safety
/// `draw` must only use vertices `0..vertex_count`.
pub unsafe fn draw_skinned(
    state: &State,
    gles: &mut dyn GLES,
    mem: &Mem,
    vertex_count: usize,
    draw: impl FnOnce(&mut dyn GLES),
) {
    if vertex_count == 0 || gles.IsEnabled(gles11::VERTEX_ARRAY) == 0 {
        draw(gles);
        return;
    }
    let with_normals = gles.IsEnabled(gles11::NORMAL_ARRAY) != 0;
    let Some((positions, normals)) = skin(state, mem, vertex_count, with_normals) else {
        log_once!("Warning: couldn't read vertex data for matrix palette skinning, drawing without skinning");
        draw(gles);
        return;
    };

    let mut array_buffer = 0;
    gles.GetIntegerv(gles11::ARRAY_BUFFER_BINDING, &mut array_buffer);
    let mut matrix_mode = 0;
    gles.GetIntegerv(gles11::MATRIX_MODE, &mut matrix_mode);

    gles.BindBuffer(gles11::ARRAY_BUFFER, 0);
    gles.VertexPointer(4, gles11::FLOAT, 0, positions.as_ptr().cast());
    if with_normals {
        gles.NormalPointer(gles11::FLOAT, 0, normals.as_ptr().cast());
    }
    // The skinned positions are already in eye space.
    gles.MatrixMode(gles11::MODELVIEW);
    gles.PushMatrix();
    gles.LoadIdentity();

    draw(gles);

    gles.PopMatrix();
    gles.MatrixMode(matrix_mode as GLenum);
    let vertex = &state.vertex_pointer;
    gles.BindBuffer(gles11::ARRAY_BUFFER, vertex.buffer);
    gles.VertexPointer(
        vertex.size,
        vertex.type_,
        vertex.stride,
        host_pointer(mem, vertex),
    );
    if with_normals {
        let normal = &state.normal_pointer;
        gles.BindBuffer(gles11::ARRAY_BUFFER, normal.buffer);
        gles.NormalPointer(normal.type_, normal.stride, host_pointer(mem, normal));
    }
    gles.BindBuffer(gles11::ARRAY_BUFFER, array_buffer as GLuint);
}
