//! The 10-20 montage a topomap places its channels on: a copy of the viewers' `eegLayout.ts`,
//! azimuthal-equidistant on the unit square with Cz at its centre, read by a channel's name.

use goofi_core::{Coord, Data};

const HEAD: &[(&str, [f32; 2])] = &[
    ("Fpz", [0.5, 0.05]), ("Fz", [0.5, 0.27]), ("Cz", [0.5, 0.5]), ("Pz", [0.5, 0.73]), ("Oz", [0.5, 0.95]),
    ("Fp1", [0.35, 0.07]), ("Fp2", [0.65, 0.07]), ("AF3", [0.36, 0.18]), ("AF4", [0.64, 0.18]),
    ("AF7", [0.27, 0.13]), ("AF8", [0.73, 0.13]), ("F3", [0.33, 0.27]), ("F4", [0.67, 0.27]),
    ("F5", [0.27, 0.27]), ("F6", [0.73, 0.27]), ("F7", [0.17, 0.25]), ("F8", [0.83, 0.25]),
    ("F1", [0.42, 0.27]), ("F2", [0.58, 0.27]), ("C3", [0.27, 0.5]), ("C4", [0.73, 0.5]),
    ("C5", [0.16, 0.5]), ("C6", [0.84, 0.5]), ("C1", [0.39, 0.5]), ("C2", [0.61, 0.5]),
    ("FC3", [0.31, 0.38]), ("FC4", [0.69, 0.38]), ("FC5", [0.22, 0.38]), ("FC6", [0.78, 0.38]),
    ("FC1", [0.41, 0.38]), ("FC2", [0.59, 0.38]), ("T7", [0.07, 0.5]), ("T8", [0.93, 0.5]),
    ("T3", [0.07, 0.5]), ("T4", [0.93, 0.5]), ("CP3", [0.31, 0.62]), ("CP4", [0.69, 0.62]),
    ("CP5", [0.22, 0.62]), ("CP6", [0.78, 0.62]), ("CP1", [0.41, 0.62]), ("CP2", [0.59, 0.62]),
    ("P3", [0.33, 0.73]), ("P4", [0.67, 0.73]), ("P5", [0.27, 0.73]), ("P6", [0.73, 0.73]),
    ("P7", [0.17, 0.75]), ("P8", [0.83, 0.75]), ("P1", [0.42, 0.73]), ("P2", [0.58, 0.73]),
    ("O1", [0.4, 0.92]), ("O2", [0.6, 0.92]), ("PO3", [0.36, 0.83]), ("PO4", [0.64, 0.83]),
    ("PO7", [0.27, 0.87]), ("PO8", [0.73, 0.87]), ("POz", [0.5, 0.83]), ("T5", [0.17, 0.75]),
    ("T6", [0.83, 0.75]), ("A1", [0.0, 0.5]), ("A2", [1.0, 0.5]),
];

/// The montage's place for a channel name, whatever its case.
fn place(name: &str) -> Option<[f32; 2]> {
    HEAD.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, p)| *p)
}

/// The electrode under each name on a frame's first axis: one `(x, y, known, 0)` texel per
/// entry in the upload's own format, as a `[N, 1]` texture; `None` where the axis has no names.
pub(crate) fn layout_of(frame: &Data) -> Option<(u32, Vec<u8>)> {
    let coords = frame.meta().channels().get(0)?.coords.as_ref()?;
    if coords.is_empty() || coords.len() > crate::plan::MAX_SIZE as usize {
        return None;
    }
    let texels = coords
        .iter()
        .flat_map(|c| match c {
            Coord::Str(name) => place(name).map_or([0.0, 0.0, 0.0, 0.0], |[x, y]| [x, y, 1.0, 0.0]),
            Coord::Num(_) => [0.0; 4],
        })
        .flat_map(|v| half::f16::from_f32(v).to_bits().to_le_bytes())
        .collect();
    Some((coords.len() as u32, texels))
}
