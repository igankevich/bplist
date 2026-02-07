use alloc::vec::Vec;

use crate::InvalidPropertyList;

/// Property list trailer.
#[derive(Debug, Clone)]
pub(crate) struct Trailer {
    /// Total number of objects.
    pub num_objects: u64,
    /// The index of the top object.
    pub top_object: u64,
    /// The offset of the offset table.
    pub offset_table_offset: u64,
    /// Offset size in bytes.
    pub offset_int_size: u8,
    /// Object reference size in bytes.
    pub object_ref_size: u8,
}

impl Trailer {
    pub const LEN: usize = 5 + 3 + 8 * 3;

    pub fn parse(b: &[u8; Self::LEN]) -> Result<Self, InvalidPropertyList> {
        let offset_int_size = b[6];
        let object_ref_size = b[7];
        let num_objects =
            u64::from_be_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]);
        let top_object =
            u64::from_be_bytes([b[16], b[17], b[18], b[19], b[20], b[21], b[22], b[23]]);
        if ![1, 2, 4, 8].contains(&offset_int_size)
            || ![1, 2, 4, 8].contains(&object_ref_size)
            || top_object >= num_objects
            || num_objects == 0
        {
            return Err(InvalidPropertyList);
        }
        Ok(Self {
            offset_int_size,
            object_ref_size,
            num_objects,
            top_object,
            offset_table_offset: u64::from_be_bytes([
                b[24], b[25], b[26], b[27], b[28], b[29], b[30], b[31],
            ]),
        })
    }

    pub fn write(&self, output: &mut Vec<u8>) {
        output.extend_from_slice(&[0; 6]);
        output.push(self.offset_int_size);
        output.push(self.object_ref_size);
        output.extend_from_slice(&self.num_objects.to_be_bytes());
        output.extend_from_slice(&self.top_object.to_be_bytes());
        output.extend_from_slice(&self.offset_table_offset.to_be_bytes());
    }
}
