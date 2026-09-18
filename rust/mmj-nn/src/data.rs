//! The experience file shared between the Rust self-play generator and the
//! Python trainer.
//!
//! ```text
//! "MMJDATA1\n"
//! one JSON header line, padded with spaces to HEADER_BYTES bytes, then '\n'
//! records, back to back, RECORD_BYTES each:
//!
//!   offset  size            field
//!   0       FEATURE_DIM     features, each `round(clamp(v,0,1) * 255)` as u8
//!   FEATURE_DIM  14         legal-action mask, bit `s` of byte `s/8`, LSB first
//!   +14     2               chosen action slot, little-endian u16
//!   +16     4               Monte-Carlo return (score delta / 1000), f32 le
//!   +20     1               seat the decision belongs to
//! ```
//!
//! The header is rewritten with the final record count when the writer is
//! finished, so the file is self-describing and streamable.

use crate::POLICY_DIM;
use crate::FEATURE_DIM;
use serde_json::{Value, json};
use std::fs::File;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

/// Magic string at the top of an experience file.
pub const DATA_MAGIC: &str = "MMJDATA1";
/// Bytes reserved for the JSON header line.
pub const HEADER_BYTES: usize = 1024;
/// Bytes of the packed action mask.
pub const MASK_BYTES: usize = POLICY_DIM.div_ceil(8);
/// Size of one record.
pub const RECORD_BYTES: usize = FEATURE_DIM + MASK_BYTES + 2 + 4 + 4 * 3 + 1;

/// Marker written into a JSON header in place of a value the writer fills in
/// later.
const PAD: u8 = b' ';

/// Streams experience records into a file.
pub struct DataWriter {
    file: BufWriter<File>,
    count: u64,
    kind: String,
    meta: Value,
    buf: Vec<u8>,
}

impl DataWriter {
    /// Create a new experience file. `kind` is `"imitation"` or `"selfplay"`.
    pub fn create(path: &Path, kind: &str, meta: Value) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = BufWriter::new(File::create(path)?);
        file.write_all(DATA_MAGIC.as_bytes())?;
        file.write_all(b"\n")?;
        // Placeholder header; rewritten by `finish`.
        let placeholder = vec![PAD; HEADER_BYTES];
        file.write_all(&placeholder)?;
        file.write_all(b"\n")?;
        Ok(DataWriter {
            file,
            count: 0,
            kind: kind.to_string(),
            meta,
            buf: Vec::with_capacity(RECORD_BYTES * 256),
        })
    }

    /// Append one decision.
    pub fn push(
        &mut self,
        features: &[f32],
        mask: &[u8],
        action_slot: usize,
        ret: f32,
        seat: u8,
    ) -> io::Result<()> {
        self.push_with_parts(features, mask, action_slot, ret, seat, OutcomeParts::default())
    }

    /// As [`DataWriter::push`], with the outcome decomposition attached.
    pub fn push_with_parts(
        &mut self,
        features: &[f32],
        mask: &[u8],
        action_slot: usize,
        ret: f32,
        seat: u8,
        parts: OutcomeParts,
    ) -> io::Result<()> {
        self.buf.clear();
        encode_record(&mut self.buf, features, mask, action_slot, ret, seat, parts);
        debug_assert_eq!(self.buf.len(), RECORD_BYTES);
        self.file.write_all(&self.buf)?;
        self.count += 1;
        Ok(())
    }

    /// Append a pre-encoded blob of whole records (used by the parallel
    /// generator, which encodes on worker threads).
    pub fn write_records(&mut self, records: &[u8]) -> io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        debug_assert_eq!(records.len() % RECORD_BYTES, 0);
        self.file.write_all(records)?;
        self.count += (records.len() / RECORD_BYTES) as u64;
        Ok(())
    }

    /// Number of records written so far.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Flush the records and rewrite the header with the real count.
    pub fn finish(mut self) -> io::Result<u64> {
        self.file.flush()?;
        let header = json!({
            "kind": self.kind,
            "feature_dim": FEATURE_DIM,
            "policy_dim": POLICY_DIM,
            "record_bytes": RECORD_BYTES,
            "mask_bytes": MASK_BYTES,
            "count": self.count,
            "meta": self.meta,
        });
        let text = serde_json::to_string(&header).unwrap();
        if text.len() > HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "experience header does not fit",
            ));
        }
        let mut file = self.file.into_inner()?;
        let mut padded = vec![PAD; HEADER_BYTES];
        padded[..text.len()].copy_from_slice(text.as_bytes());
        file.seek(SeekFrom::Start(DATA_MAGIC.len() as u64 + 1))?;
        file.write_all(&padded)?;
        file.flush()?;
        Ok(self.count)
    }
}

/// Append one encoded record to `out`.
/// Outcome components of one hand, stored beside the raw return so the value
/// head can be trained on low-variance targets instead of the whole hand swing.
///
/// The hand-scoped return has a standard deviation of about 4.7 thousand points
/// and is dominated by luck: the best single-head model this project trained
/// explains only 14% of it. Decomposing it into "did I win, and for how much" and
/// "did I deal in, and for how much" gives pieces whose variance is an order of
/// magnitude smaller, and they recombine into the same expectation.
#[derive(Clone, Copy, Debug, Default)]
pub struct OutcomeParts {
    /// Points gained from other players by winning this hand (0 otherwise).
    pub won_value: f32,
    /// Points paid to other players by dealing in (0 otherwise).
    pub dealt_value: f32,
    /// Everything else: riichi sticks, tenpai payments at an exhaustive draw.
    pub other_value: f32,
}

pub fn encode_record(
    out: &mut Vec<u8>,
    features: &[f32],
    mask: &[u8],
    action_slot: usize,
    ret: f32,
    seat: u8,
    parts: OutcomeParts,
) {
    debug_assert_eq!(features.len(), FEATURE_DIM);
    for &v in features.iter() {
        out.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    let mut packed = [0u8; MASK_BYTES];
    for (i, &m) in mask.iter().enumerate() {
        if m == 1 && i < POLICY_DIM {
            packed[i / 8] |= 1 << (i % 8);
        }
    }
    out.extend_from_slice(&packed);
    out.extend_from_slice(&(action_slot as u16).to_le_bytes());
    out.extend_from_slice(&ret.to_le_bytes());
    out.extend_from_slice(&parts.won_value.to_le_bytes());
    out.extend_from_slice(&parts.dealt_value.to_le_bytes());
    out.extend_from_slice(&parts.other_value.to_le_bytes());
    out.push(seat);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_layout_is_stable() {
        assert_eq!(MASK_BYTES, 14);
        // Round 19 added the outcome decomposition (3 floats) so the value head
        // can be trained on low-variance pieces. Changing this number is a data
        // format break: every existing experience file becomes unreadable.
        assert_eq!(RECORD_BYTES, FEATURE_DIM + 14 + 2 + 4 + 12 + 1);
    }

    #[test]
    fn encode_matches_writer() {
        let features = vec![0.25f32; FEATURE_DIM];
        let mut mask = vec![0u8; POLICY_DIM];
        mask[3] = 1;
        let mut blob = Vec::new();
        encode_record(&mut blob, &features, &mask, 3, 2.0, 1, OutcomeParts::default());
        assert_eq!(blob.len(), RECORD_BYTES);
        assert_eq!(blob[0], 64, "0.25 * 255");
    }

    #[test]
    fn writer_roundtrip() {
        let path = std::env::temp_dir().join("mmj-data-test.bin");
        let mut w = DataWriter::create(&path, "imitation", json!({"seed": 1})).unwrap();
        let features = vec![0.5f32; FEATURE_DIM];
        let mut mask = vec![0u8; POLICY_DIM];
        mask[7] = 1;
        mask[40] = 1;
        w.push(&features, &mask, 40, -1.25, 2).unwrap();
        w.push(&features, &mask, 7, 3.5, 0).unwrap();
        assert_eq!(w.finish().unwrap(), 2);

        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(DATA_MAGIC.as_bytes()));
        let header_end = DATA_MAGIC.len() + 1 + HEADER_BYTES + 1;
        let header: Value =
            serde_json::from_slice(&bytes[DATA_MAGIC.len() + 1..DATA_MAGIC.len() + 1 + HEADER_BYTES])
                .unwrap();
        assert_eq!(header["count"], 2);
        assert_eq!(bytes.len(), header_end + 2 * RECORD_BYTES);
        // First record.
        let r0 = &bytes[header_end..header_end + RECORD_BYTES];
        assert_eq!(r0[0], 128, "0.5 * 255 rounds to 128");
        assert_eq!(r0[FEATURE_DIM + 40 / 8] & (1 << (40 % 8)), 1 << 0);
        let slot = u16::from_le_bytes([r0[FEATURE_DIM + 14], r0[FEATURE_DIM + 15]]);
        assert_eq!(slot, 40);
        let ret = f32::from_le_bytes([
            r0[FEATURE_DIM + 16],
            r0[FEATURE_DIM + 17],
            r0[FEATURE_DIM + 18],
            r0[FEATURE_DIM + 19],
        ]);
        assert_eq!(ret, -1.25);
        assert_eq!(r0[RECORD_BYTES - 1], 2);
    }
}
