use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

pub struct State {
    pub dump: Mutex<BufWriter<File>>,
    pub sample_period_bytes: usize,
}

pub static STATE: LazyLock<Option<State>> = LazyLock::new(|| {
    Some(State {
        dump: {
            let cap = env::var("TRACE_ALLOC_DUMP_BUF_SIZE_BYTES").ok()?;
            let cap = str::parse::<usize>(&cap).ok()?;

            let path = env::var("TRACE_ALLOC_DUMP_PATH").ok()?;
            let path = Path::new(&path);
            let file = File::create(path).ok()?;
            let file = BufWriter::with_capacity(cap, file);

            Mutex::new(file)
        },
        sample_period_bytes: {
            let bytes = env::var("TRACE_ALLOC_SAMPLE_PERIOD_BYTES").ok()?;
            str::parse::<usize>(&bytes).ok()?
        },
    })
});
