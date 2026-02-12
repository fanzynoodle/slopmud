//! Manually written FlatBuffers schema bindings for websocket frames.
//!
//! Schema (ws_frame.fbs):
//! table Frame { t:ubyte; session:[ubyte] (length:16); body:[ubyte]; }
//! root_type Frame;

use flatbuffers::FlatBufferBuilder;
use flatbuffers::Follow;
use flatbuffers::ForwardsUOffset;
use flatbuffers::Table;
use flatbuffers::TableUnfinishedWIPOffset;
use flatbuffers::VOffsetT;
use flatbuffers::Vector;
use flatbuffers::WIPOffset;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Frame<'a> {
    _tab: Table<'a>,
}

impl<'a> Follow<'a> for Frame<'a> {
    type Inner = Frame<'a>;
    unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        Frame {
            _tab: unsafe { Table::new(buf, loc) },
        }
    }
}

impl<'a> Frame<'a> {
    const VT_T: VOffsetT = 4;
    const VT_SESSION: VOffsetT = 6;
    const VT_BODY: VOffsetT = 8;

    pub fn t(&self) -> u8 {
        unsafe { self._tab.get::<u8>(Self::VT_T, Some(0)).unwrap_or(0) }
    }

    pub fn session(&self) -> Option<Vector<'a, u8>> {
        unsafe {
            self._tab
                .get::<ForwardsUOffset<Vector<'a, u8>>>(Self::VT_SESSION, None)
        }
    }

    pub fn body(&self) -> Option<Vector<'a, u8>> {
        unsafe {
            self._tab
                .get::<ForwardsUOffset<Vector<'a, u8>>>(Self::VT_BODY, None)
        }
    }
}

#[derive(Default)]
pub struct FrameArgs<'a> {
    pub t: u8,
    pub session: Option<WIPOffset<Vector<'a, u8>>>,
    pub body: Option<WIPOffset<Vector<'a, u8>>>,
}

pub struct FrameBuilder<'a: 'b, 'b> {
    fbb: &'b mut FlatBufferBuilder<'a>,
    start: WIPOffset<TableUnfinishedWIPOffset>,
}

impl<'a: 'b, 'b> FrameBuilder<'a, 'b> {
    pub fn new(fbb: &'b mut FlatBufferBuilder<'a>) -> Self {
        let start = fbb.start_table();
        Self { fbb, start }
    }

    pub fn add_t(&mut self, t: u8) {
        self.fbb.push_slot::<u8>(Frame::VT_T, t, 0);
    }

    pub fn add_session(&mut self, session: WIPOffset<Vector<'b, u8>>) {
        self.fbb.push_slot_always(Frame::VT_SESSION, session);
    }

    pub fn add_body(&mut self, body: WIPOffset<Vector<'b, u8>>) {
        self.fbb.push_slot_always(Frame::VT_BODY, body);
    }

    pub fn finish(self) -> WIPOffset<Frame<'a>> {
        let o = self.fbb.end_table(self.start);
        WIPOffset::new(o.value())
    }
}

pub fn create_frame<'a: 'b, 'b>(
    fbb: &'b mut FlatBufferBuilder<'a>,
    args: &FrameArgs<'b>,
) -> WIPOffset<Frame<'a>> {
    let mut b = FrameBuilder::new(fbb);
    b.add_t(args.t);
    if let Some(s) = args.session {
        b.add_session(s);
    }
    if let Some(body) = args.body {
        b.add_body(body);
    }
    b.finish()
}

pub fn finish_frame_buf(t: u8, session: [u8; 16], body: &[u8]) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let sess_v = fbb.create_vector(&session);
    let body_v = fbb.create_vector(body);
    let fr = create_frame(
        &mut fbb,
        &FrameArgs {
            t,
            session: Some(sess_v),
            body: Some(body_v),
        },
    );
    fbb.finish(fr, None);
    fbb.finished_data().to_vec()
}
