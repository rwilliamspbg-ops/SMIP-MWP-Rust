// wire crate: protocol types and parsing for SMIP

pub const HEADER_SIZE: usize = 96; // matches Go implementation

// MCR spray flags (stored in the existing `flags` field)
pub const MCR_SPRAY_FLAG: u16 = 1 << 0;
pub const MCR_FULL_SPRAY_FLAG: u16 = 1 << 1;

const SRC_OFFSET: usize = 0;
const DST_OFFSET: usize = 32;
const FLOW_OFFSET: usize = 64;
const SEQ_OFFSET: usize = 68;
const SESSION_OFFSET: usize = 76;
const FLAGS_OFFSET: usize = 92;
const LEN_OFFSET: usize = 94;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub src_id: [u8; 32],
    pub dst_id: [u8; 32],
    pub flow_label: u32,
    pub seq_num: u64,
    pub session_id: [u8; 16],
    pub flags: u16,
    pub length: u16,
}

#[derive(thiserror::Error, Debug)]
#[error("wire: buffer too small for header")]
pub struct ErrBufferTooSmall;

impl Header {
    pub fn new() -> Self {
        Self {
            src_id: [0; 32],
            dst_id: [0; 32],
            flow_label: 0,
            seq_num: 0,
            session_id: [0; 16],
            flags: 0,
            length: 0,
        }
    }

    pub fn marshal_into(&self, buf: &mut [u8]) -> Result<(), ErrBufferTooSmall> {
        if buf.len() < HEADER_SIZE {
            return Err(ErrBufferTooSmall);
        }
        // Direct pointer cast to &[u8; HEADER_SIZE] after length check avoids slice subslicing and try_into bounds checks
        let buf_ptr = buf.as_mut_ptr();
        unsafe {
            *(buf_ptr.add(SRC_OFFSET) as *mut [u8; 32]) = self.src_id;
            *(buf_ptr.add(DST_OFFSET) as *mut [u8; 32]) = self.dst_id;
            *(buf_ptr.add(FLOW_OFFSET) as *mut [u8; 4]) = self.flow_label.to_be_bytes();
            *(buf_ptr.add(SEQ_OFFSET) as *mut [u8; 8]) = self.seq_num.to_be_bytes();
            *(buf_ptr.add(SESSION_OFFSET) as *mut [u8; 16]) = self.session_id;
            *(buf_ptr.add(FLAGS_OFFSET) as *mut [u8; 2]) = self.flags.to_be_bytes();
            *(buf_ptr.add(LEN_OFFSET) as *mut [u8; 2]) = self.length.to_be_bytes();
        }
        Ok(())
    }

    pub fn parse(buf: &[u8]) -> Result<Self, ErrBufferTooSmall> {
        if buf.len() < HEADER_SIZE {
            return Err(ErrBufferTooSmall);
        }
        // Direct pointer reads from validated buffer avoid slice subslicing and try_into conversions
        let buf_ptr = buf.as_ptr();
        unsafe {
            let src_id = *(buf_ptr.add(SRC_OFFSET) as *const [u8; 32]);
            let dst_id = *(buf_ptr.add(DST_OFFSET) as *const [u8; 32]);
            let flow_label = u32::from_be_bytes(*(buf_ptr.add(FLOW_OFFSET) as *const [u8; 4]));
            let seq_num = u64::from_be_bytes(*(buf_ptr.add(SEQ_OFFSET) as *const [u8; 8]));
            let session_id = *(buf_ptr.add(SESSION_OFFSET) as *const [u8; 16]);
            let flags = u16::from_be_bytes(*(buf_ptr.add(FLAGS_OFFSET) as *const [u8; 2]));
            let length = u16::from_be_bytes(*(buf_ptr.add(LEN_OFFSET) as *const [u8; 2]));
            Ok(Header {
                src_id,
                dst_id,
                flow_label,
                seq_num,
                session_id,
                flags,
                length,
            })
        }
    }

    pub fn new_header_buffer(payload_len: usize) -> Vec<u8> {
        vec![0u8; HEADER_SIZE + payload_len]
    }
}

impl Default for Header {
    fn default() -> Self {
        Self::new()
    }
}

/// Zero-copy immutable view into a packet buffer — no field copies, no
/// struct allocation.  Use this on the read-only hot path instead of
/// `Header::parse()`.
pub struct HeaderViewRef<'a> {
    buf: &'a [u8; HEADER_SIZE],
}

impl<'a> HeaderViewRef<'a> {
    #[inline]
    pub fn new(buf: &'a [u8]) -> Result<Self, ErrBufferTooSmall> {
        if buf.len() < HEADER_SIZE {
            return Err(ErrBufferTooSmall);
        }
        // Direct raw pointer cast to &'a [u8; HEADER_SIZE] after length check bypasses subslicing and try_into unwrapping
        let buf = unsafe { &*(buf.as_ptr() as *const [u8; HEADER_SIZE]) };
        Ok(Self { buf })
    }
    // Direct pointer casting from validated [u8; HEADER_SIZE] buffer bypasses slice fat pointer creation, element-by-element indexing, and try_from length assertions.
    #[inline]
    pub fn src_id(&self) -> &[u8; 32] {
        unsafe { &*(self.buf.as_ptr().add(SRC_OFFSET) as *const [u8; 32]) }
    }
    #[inline]
    pub fn dst_id(&self) -> &[u8; 32] {
        unsafe { &*(self.buf.as_ptr().add(DST_OFFSET) as *const [u8; 32]) }
    }
    #[inline]
    pub fn flow_label(&self) -> u32 {
        u32::from_be_bytes(unsafe { *(self.buf.as_ptr().add(FLOW_OFFSET) as *const [u8; 4]) })
    }
    #[inline]
    pub fn seq_num(&self) -> u64 {
        u64::from_be_bytes(unsafe { *(self.buf.as_ptr().add(SEQ_OFFSET) as *const [u8; 8]) })
    }
    #[inline]
    pub fn session_id(&self) -> &[u8; 16] {
        unsafe { &*(self.buf.as_ptr().add(SESSION_OFFSET) as *const [u8; 16]) }
    }
    #[inline]
    pub fn flags(&self) -> u16 {
        u16::from_be_bytes(unsafe { *(self.buf.as_ptr().add(FLAGS_OFFSET) as *const [u8; 2]) })
    }
    #[inline]
    pub fn length(&self) -> u16 {
        u16::from_be_bytes(unsafe { *(self.buf.as_ptr().add(LEN_OFFSET) as *const [u8; 2]) })
    }
}

pub struct HeaderView<'a> {
    buf: &'a mut [u8; HEADER_SIZE],
}

impl<'a> HeaderView<'a> {
    #[inline]
    pub fn view(buf: &'a mut [u8]) -> Result<Self, ErrBufferTooSmall> {
        if buf.len() < HEADER_SIZE {
            return Err(ErrBufferTooSmall);
        }
        // Direct raw pointer cast to &'a mut [u8; HEADER_SIZE] after length check bypasses subslicing and try_into unwrapping
        let buf = unsafe { &mut *(buf.as_mut_ptr() as *mut [u8; HEADER_SIZE]) };
        Ok(Self { buf })
    }
    // Direct pointer casting from validated [u8; HEADER_SIZE] buffer bypasses slice fat pointer creation, element-by-element indexing, and try_from length assertions.
    #[inline]
    pub fn src_id(&self) -> &[u8; 32] {
        unsafe { &*(self.buf.as_ptr().add(SRC_OFFSET) as *const [u8; 32]) }
    }
    #[inline]
    pub fn dst_id(&self) -> &[u8; 32] {
        unsafe { &*(self.buf.as_ptr().add(DST_OFFSET) as *const [u8; 32]) }
    }
    #[inline]
    pub fn flow_label(&self) -> u32 {
        u32::from_be_bytes(unsafe { *(self.buf.as_ptr().add(FLOW_OFFSET) as *const [u8; 4]) })
    }
    #[inline]
    pub fn seq_num(&self) -> u64 {
        u64::from_be_bytes(unsafe { *(self.buf.as_ptr().add(SEQ_OFFSET) as *const [u8; 8]) })
    }
    #[inline]
    pub fn session_id(&self) -> &[u8; 16] {
        unsafe { &*(self.buf.as_ptr().add(SESSION_OFFSET) as *const [u8; 16]) }
    }
    #[inline]
    pub fn flags(&self) -> u16 {
        u16::from_be_bytes(unsafe { *(self.buf.as_ptr().add(FLAGS_OFFSET) as *const [u8; 2]) })
    }
    #[inline]
    pub fn length(&self) -> u16 {
        u16::from_be_bytes(unsafe { *(self.buf.as_ptr().add(LEN_OFFSET) as *const [u8; 2]) })
    }

    // Setters - direct unaligned pointer writes into validated [u8; HEADER_SIZE] buffer bypass slice creation and copy_from_slice bounds checks.
    #[inline]
    pub fn set_src_id(&mut self, id: [u8; 32]) {
        unsafe {
            *(self.buf.as_mut_ptr().add(SRC_OFFSET) as *mut [u8; 32]) = id;
        }
    }
    #[inline]
    pub fn set_dst_id(&mut self, id: [u8; 32]) {
        unsafe {
            *(self.buf.as_mut_ptr().add(DST_OFFSET) as *mut [u8; 32]) = id;
        }
    }
    #[inline]
    pub fn set_flow_label(&mut self, v: u32) {
        unsafe {
            *(self.buf.as_mut_ptr().add(FLOW_OFFSET) as *mut [u8; 4]) = v.to_be_bytes();
        }
    }
    #[inline]
    pub fn set_seq_num(&mut self, v: u64) {
        unsafe {
            *(self.buf.as_mut_ptr().add(SEQ_OFFSET) as *mut [u8; 8]) = v.to_be_bytes();
        }
    }
    #[inline]
    pub fn set_session_id(&mut self, id: [u8; 16]) {
        unsafe {
            *(self.buf.as_mut_ptr().add(SESSION_OFFSET) as *mut [u8; 16]) = id;
        }
    }
    #[inline]
    pub fn set_flags(&mut self, v: u16) {
        unsafe {
            *(self.buf.as_mut_ptr().add(FLAGS_OFFSET) as *mut [u8; 2]) = v.to_be_bytes();
        }
    }
    #[inline]
    pub fn set_length(&mut self, v: u16) {
        unsafe {
            *(self.buf.as_mut_ptr().add(LEN_OFFSET) as *mut [u8; 2]) = v.to_be_bytes();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    fn random32() -> [u8; 32] {
        let mut b = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut b);
        b
    }

    fn random16() -> [u8; 16] {
        let mut b = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut b);
        b
    }

    #[test]
    fn marshal_and_parse() {
        let src = random32();
        let dst = random32();
        let sid = random16();

        let h = Header {
            src_id: src,
            dst_id: dst,
            flow_label: 0xdeadbeef,
            seq_num: 42,
            session_id: sid,
            flags: 0x1,
            length: 128,
        };

        let mut buf = Header::new_header_buffer(h.length as usize);
        h.marshal_into(&mut buf).unwrap();

        let parsed = Header::parse(&buf).unwrap();
        assert_eq!(parsed.src_id, h.src_id);
        assert_eq!(parsed.dst_id, h.dst_id);
        assert_eq!(parsed.flow_label, h.flow_label);
        assert_eq!(parsed.seq_num, h.seq_num);
        assert_eq!(parsed.length, h.length);
    }

    #[test]
    fn header_view_round_trip_and_bounds() {
        let src = [1u8; 32];
        let dst = [2u8; 32];
        let sid = [3u8; 16];
        let h = Header {
            src_id: src,
            dst_id: dst,
            flow_label: 0x11223344,
            seq_num: 0x55667788,
            session_id: sid,
            flags: 0x9,
            length: 64,
        };

        let mut buf = Header::new_header_buffer(h.length as usize);
        h.marshal_into(&mut buf).unwrap();

        let mut view = HeaderView::view(&mut buf).unwrap();
        assert_eq!(view.src_id(), &src);
        assert_eq!(view.dst_id(), &dst);
        assert_eq!(view.flow_label(), h.flow_label);
        assert_eq!(view.seq_num(), h.seq_num);
        assert_eq!(view.session_id(), &sid);
        assert_eq!(view.flags(), h.flags);
        assert_eq!(view.length(), h.length);

        view.set_flags(0x44);
        view.set_length(128);
        assert_eq!(Header::parse(&buf).unwrap().flags, 0x44);
        assert_eq!(Header::parse(&buf).unwrap().length, 128);
    }

    #[test]
    fn rejects_small_buffer() {
        let mut buf = vec![0u8; HEADER_SIZE - 1];
        assert!(Header::parse(&buf).is_err());
        assert!(HeaderView::view(&mut buf).is_err());
    }
}
