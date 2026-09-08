//! Which of a device's channels a patch means, written the way an interface labels its sockets.
//!
//! WASAPI publishes an interface as one stereo endpoint per pair, so "which channels" was never a
//! question a patch could ask: the endpoint WAS the answer. ASIO hands over the whole card at once
//! — eighteen inputs and eight outputs on a Scarlett — and the question becomes unavoidable. This
//! is the one place it is answered, for `AudioIn` and `AudioOut` alike, so the two spell it the
//! same and a patch reads the same on either node.
//!
//! The spelling is the front panel's: `1`, `2`, `1-2`, `3-4`, `1,3-4`. ONE-BASED, because that is
//! what is silkscreened next to the socket, and a selection written `0-1` for the first pair would
//! be right in the code and wrong at the desk. `all` — the default — is the whole device, and is
//! what every patch written before this param meant.
//!
//! A range may count down: `4-3` is channel 4 then channel 3, which is how a pair is swapped
//! without a node in between. Order is kept exactly as written, so a selection is a PATCHBAY and
//! not a set — `3-4` and `4-3` are different answers, and duplicates are allowed because sending
//! one source to two destinations is a real thing to want.

use goofi_audio_sdk::MAX_CHANNELS;

/// What a `channels` param means when it names the whole device.
pub const ALL: &str = "all";

/// The 0-based device channels `spec` names, in order, or `None` for the whole device.
///
/// `Err` is a message for the param, so it says what was wrong AND what the spelling is: a
/// selection is typed by hand, and the common mistakes — a zero, a bare dash, a stray letter —
/// are all ones a person makes once and then never again once told.
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
    if out.len() > MAX_CHANNELS as usize {
        return Err(format!(
            "`{spec}` selects {} channels and the engine carries {MAX_CHANNELS}",
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

/// The doc every `channels` param carries, so the two nodes say the same thing.
pub const DOC: &str = "which of the device's channels to use, counting from 1 as the device labels \
them: `1`, `1-2`, `3-4`, `1,3-4`, or `all` for every channel. A range may count down — `4-3` is \
channel 4 then channel 3 — and the order written is the order used. WASAPI publishes a card as \
stereo endpoints, so a selection past `1-2` usually means naming the card's `ASIO: ` device.";

#[cfg(test)]
mod tests {
    use super::{needed_width, parse};

    #[test]
    fn all_is_the_whole_device() {
        assert_eq!(parse("all").unwrap(), None);
        assert_eq!(parse("  ALL ").unwrap(), None);
        assert_eq!(parse("").unwrap(), None);
    }

    /// The front panel's numbers, and the buffer's: the off-by-one is the point of the type.
    #[test]
    fn a_selection_is_one_based_at_the_desk_and_zero_based_in_the_buffer() {
        assert_eq!(parse("1").unwrap(), Some(vec![0]));
        assert_eq!(parse("2").unwrap(), Some(vec![1]));
        assert_eq!(parse("1-2").unwrap(), Some(vec![0, 1]));
        assert_eq!(parse("3-4").unwrap(), Some(vec![2, 3]));
        assert_eq!(parse("1,3-4").unwrap(), Some(vec![0, 2, 3]));
        assert_eq!(parse(" 3 , 1 ").unwrap(), Some(vec![2, 0]));
    }

    /// A patchbay and not a set: order is kept, a range may count down, and a repeat is a fan-out.
    #[test]
    fn order_is_kept_and_a_range_may_count_down() {
        assert_eq!(parse("4-3").unwrap(), Some(vec![3, 2]));
        assert_eq!(parse("2,1").unwrap(), Some(vec![1, 0]));
        assert_eq!(parse("1,1").unwrap(), Some(vec![0, 0]));
    }

    #[test]
    fn what_is_refused_says_why() {
        for bad in ["0", "1-0", "-", "1-", "abc", "1,,2", "1-2-3", "1,"] {
            let why = parse(bad).unwrap_err();
            assert!(why.contains(bad.trim()) || why.contains("empty"), "`{bad}` refused without naming itself: {why}");
        }
        assert!(parse("0").unwrap_err().contains("count from 1"), "a zero is told where counting starts");
        assert!(parse("1-64").unwrap_err().contains("16"), "past the engine's width the ceiling is named");
    }

    /// A channel is only reachable if the stream was opened wide enough to hold it — the reason
    /// `17-18` on an eighteen-in card works at all.
    #[test]
    fn the_width_needed_is_one_past_the_highest() {
        assert_eq!(needed_width(&[0, 1]), 2);
        assert_eq!(needed_width(&[2, 3]), 4);
        assert_eq!(needed_width(&[16, 17]), 18);
        assert_eq!(needed_width(&[]), 1);
    }
}
