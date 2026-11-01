use std::mem;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use crate::dump::record::{Record, RecordStream};

mod record;

pub struct Sample {
    pub counts: Counts,
    pub loaded_elfs: Arc<Vec<(u64, PathBuf)>>,
    pub call_chain: Vec<u64>,
}

pub struct Counts {
    pub acc_bytes: u64,
    pub alive_bytes: usize,
    pub peak_alive_bytes: usize,
    pub realloc_growth_count: u64,
    pub realloc_shrink_count: u64,
}

pub struct SampleStream<'a, S> {
    loaded_elfs: LoadedELfs,
    stream: RecordStream<'a, S>,
}

enum LoadedELfs {
    Shared(Arc<Vec<(u64, PathBuf)>>),
    Owned(Vec<(u64, PathBuf)>),
}

impl<'a, S> SampleStream<'a, S>
where
    S: Iterator<Item = u8>,
{
    pub fn new(s: &'a mut S) -> Self {
        Self {
            loaded_elfs: LoadedELfs::Owned(vec![]),
            stream: RecordStream::new(s),
        }
    }
}

impl<'a, S> Iterator for SampleStream<'a, S>
where
    S: Iterator<Item = u8>,
{
    type Item = Result<Sample, Range<u64>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut counts = None;

        let stream = &mut self.stream;
        let start = stream.offset();

        loop {
            match (counts, stream.next()?) {
                (None, Ok(Record::C(c))) => {
                    counts = Some(Counts {
                        acc_bytes: c[0],
                        alive_bytes: c[1] as _,
                        peak_alive_bytes: c[2] as _,
                        realloc_growth_count: c[3],
                        realloc_shrink_count: c[4],
                    });
                }
                (Some(counts), Ok(Record::A(call_chain))) => {
                    let loaded_elfs = match &mut self.loaded_elfs {
                        LoadedELfs::Shared(s) => Arc::clone(s),
                        LoadedELfs::Owned(o) => {
                            let shared = Arc::new(mem::take(o));
                            self.loaded_elfs = LoadedELfs::Shared(Arc::clone(&shared));
                            shared
                        }
                    };
                    return Some(Ok(Sample {
                        counts,
                        loaded_elfs,
                        call_chain,
                    }));
                }
                (None, Ok(Record::E(offset, path))) => {
                    counts = None;
                    match &mut self.loaded_elfs {
                        LoadedELfs::Owned(o) => {
                            o.push((offset, path));
                        }
                        _ => self.loaded_elfs = LoadedELfs::Owned(vec![(offset, path)]),
                    }
                }
                (_, Err(range)) => return Some(Err(range)),
                _ => return Some(Err(start..stream.offset())),
            }
        }
    }
}
