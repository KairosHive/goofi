//! Which of a device's channels a patch means, for `AudioIn` and `AudioOut` alike: a 1-based,
//! ordered selection that may repeat a channel, or `all` for the whole device.

use goofi_audio_sdk::{ParamDecl, ParamSpec};

use crate::plan::CEILING;

/// What a `channels` param means when it names the whole device.
pub const ALL: &str = "all";

/// The 0-based device channels `spec` names, in order, or `None` for the whole device. `Err` is a
/// message for the param: what was wrong, and how a selection is spelled.
pub fn parse(spec: &str) -> Result<Option<Vec<u16>>, String> {
    let spec = spec.trim();
    if spec.is_empty() || spec.eq_ignore_ascii_case(ALL) {
        return Ok(None);
    }
    let mut out: Vec<u16> = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!("`{spec}`: an empty selection between commas; write `1,3-4` or `{ALL}`"));
        }
        let one = |n: &str| -> Result<u16, String> {
            match n.trim().parse::<u16>() {
                Ok(0) => Err(format!("`{spec}`: channels count from 1, as they do on the device")),
                Ok(n) => Ok(n),
                Err(_) => Err(format!("`{spec}`: `{n}` is not a channel number; write `1`, `1-2`, `1,3-4` or `{ALL}`")),
            }
        };
        // `split_once` and not `split`, so a range is exactly two numbers and `1-2-3` is refused
        // rather than silently read as its first pair.
        match part.split_once('-') {
            Some((lo, hi)) => {
                let (lo, hi) = (one(lo)?, one(hi)?);
                // Counting down is deliberate — `4-3` swaps a pair — so the range is walked in
                // whichever direction it was written.
                if lo <= hi {
                    out.extend(lo..=hi);
                } else {
                    out.extend((hi..=lo).rev());
                }
            }
            None => out.push(one(part)?),
        }
    }
    if out.len() > CEILING as usize {
        return Err(format!(
            "`{spec}` selects {} channels and a port carries {CEILING}",
            out.len()
        ));
    }
    // 1-based at the desk, 0-based in the buffer, and this subtraction is the whole of the border.
    Ok(Some(out.into_iter().map(|c| c - 1).collect()))
}

/// The device width a selection needs open: one past its highest channel, since a channel is only
/// reachable if the stream was opened wide enough to contain it.
pub fn needed_width(sel: &[u16]) -> u16 {
    sel.iter().copied().max().map_or(1, |c| c + 1)
}

/// The `channels` param both device nodes carry, so the two spell a selection the same.
pub const PARAM: ParamDecl = ParamDecl {
    group: "audio",
    name: "channels",
    spec: ParamSpec::Str { default: ALL, options: &[], refresh: false },
    expression: None,
    doc: Some(
        "which of the device's channels to use, counting from 1 as the device labels \
them: `1`, `1-2`, `3-4`, `1,3-4`, or `all` for every channel. A range may count down — `4-3` is \
channel 4 then channel 3 — and the order written is the order used. WASAPI publishes a card as \
stereo endpoints, so a selection past `1-2` usually means naming the card's `ASIO: ` device.",
    ),
    section: 0,
    show: None,
    role: None,
};
