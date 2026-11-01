use std::ffi::OsString;
use std::ops::Range;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

struct Offset<S> {
    stream: S,
    offset: u64,
}

impl<S> Iterator for Offset<S>
where
    S: Iterator<Item = u8>,
{
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        let b = self.stream.next()?;
        self.offset += 1;
        Some(b)
    }
}

pub enum Record {
    E(u64, PathBuf),
    A(Vec<u64>),
    C([u64; 5]),
}

enum State {
    Pending,
    HeadE,
    HeadC,
    HeadA,
    Error(Range<u64>),
}

pub struct RecordStream<'a, S> {
    stream: Offset<&'a mut S>,
    state: State,
}

impl<'a, S> RecordStream<'a, S> {
    pub fn new(s: &'a mut S) -> Self {
        Self {
            stream: Offset {
                stream: s,
                offset: 0,
            },
            state: State::Pending,
        }
    }

    pub fn offset(&self) -> u64 {
        self.stream.offset
    }
}

impl<'a, S> Iterator for RecordStream<'a, S>
where
    S: Iterator<Item = u8>,
{
    type Item = Result<Record, Range<u64>>;

    fn next(&mut self) -> Option<Self::Item> {
        let stream = &mut self.stream;
        loop {
            match &mut self.state {
                State::HeadC => {
                    let mut counts = [0; 5];
                    let mut current_count = 0;
                    let mut buf = [0; 8];

                    while current_count < counts.len() {
                        let err_start = stream.offset;
                        for b in &mut buf {
                            let Some(byte) = stream.next() else {
                                self.state = State::Pending;
                                return Some(Err(err_start..stream.offset));
                            };
                            *b = byte;
                        }
                        let count = u64::from_le_bytes(buf);

                        counts[current_count] = count;
                        current_count += 1;
                    }

                    self.state = State::Pending;
                    return Some(Ok(Record::C(counts)));
                }
                State::HeadA => {
                    let mut buf = [0; 8];

                    let len = {
                        let err_start = stream.offset;
                        for b in &mut buf {
                            let Some(byte) = stream.next() else {
                                self.state = State::Pending;
                                return Some(Err(err_start..stream.offset));
                            };
                            *b = byte;
                        }
                        u64::from_le_bytes(buf)
                    };

                    let mut addrs = Vec::with_capacity(len as _);
                    for _ in 0..len {
                        let err_start = stream.offset;
                        for b in &mut buf {
                            let Some(byte) = stream.next() else {
                                self.state = State::Pending;
                                return Some(Err(err_start..stream.offset));
                            };
                            *b = byte;
                        }
                        let addr = u64::from_le_bytes(buf);
                        addrs.push(addr);
                    }

                    self.state = State::Pending;
                    return Some(Ok(Record::A(addrs)));
                }
                State::HeadE => {
                    let offset = {
                        let err_start = stream.offset;
                        let mut buf = [0; 8];
                        for b in &mut buf {
                            let Some(byte) = stream.next() else {
                                self.state = State::Pending;
                                return Some(Err(err_start..stream.offset));
                            };
                            *b = byte;
                        }
                        u64::from_le_bytes(buf)
                    };

                    let len = {
                        let err_start = stream.offset;
                        let mut buf = [0; 2];
                        for b in &mut buf {
                            let Some(byte) = stream.next() else {
                                self.state = State::Pending;
                                return Some(Err(err_start..stream.offset));
                            };
                            *b = byte;
                        }
                        u16::from_le_bytes(buf)
                    };

                    let mut path = Vec::with_capacity(len as _);
                    let err_start = stream.offset;
                    for _ in 0..len {
                        let Some(byte) = stream.next() else {
                            self.state = State::Pending;
                            return Some(Err(err_start..stream.offset));
                        };
                        path.push(byte);
                    }
                    let path = PathBuf::from(OsString::from_vec(path));

                    self.state = State::Pending;
                    return Some(Ok(Record::E(offset, path)));
                }
                State::Pending => {
                    self.state = match stream.next()? {
                        b'c' => State::HeadC,
                        b'a' => State::HeadA,
                        b'e' => State::HeadE,
                        _ => {
                            let end = stream.offset;
                            State::Error(end - 1..end)
                        }
                    };
                }
                State::Error(range) => {
                    let b = stream.next();
                    range.end = stream.offset;
                    let Some(b) = b else {
                        let range = range.clone();
                        self.state = State::Pending;
                        return Some(Err(range));
                    };
                    match b {
                        b'c' => {
                            let range = range.clone();
                            self.state = State::HeadC;
                            return Some(Err(range));
                        }
                        b'a' => {
                            let range = range.clone();
                            self.state = State::HeadA;
                            return Some(Err(range));
                        }
                        b'e' => {
                            let range = range.clone();
                            self.state = State::HeadE;
                            return Some(Err(range));
                        }
                        _ => {}
                    };
                }
            }
        }
    }
}
