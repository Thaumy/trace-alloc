use std::borrow::Cow;
use std::fs;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use addr2line::{Context, gimli};
use clap::Parser;
use object::{Object, ObjectSection, ObjectSegment, ObjectSymbol};
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};
use trace_alloc::dump::SampleStream;

type Reader = gimli::EndianArcSlice<gimli::RunTimeEndian>;

#[derive(Serialize)]
struct Sample {
    count: Count,
    call_chain: Vec<String>,
}

#[derive(Clone, Copy, Serialize)]
struct Count {
    acc_bytes: u64,
    alive_bytes: usize,
    peak_alive_bytes: usize,
    realloc_growth_count: u64,
    realloc_shrink_count: u64,
}

struct Module {
    context: Option<Context<Reader>>,
    symbols: Vec<(u64, u64, String)>,
    segments: Vec<(u64, u64)>,
    base: Option<u64>,
}

struct Symbolizer {
    modules: Vec<Module>,
}

#[derive(Parser)]
#[command(version, about = "Symbolize trace-alloc sample dumps")]
struct Args {
    #[arg(value_name = "DUMP")]
    dump: PathBuf,
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    let mut bytes = byte_stream(&args.dump);
    let mut loaded_elfs = None;
    let mut symbolizer = Symbolizer {
        modules: Vec::new(),
    };

    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    {
        let mut serializer = serde_json::Serializer::pretty(&mut writer);
        let mut sequence = serializer.serialize_seq(None).unwrap();
        for sample in SampleStream::new(&mut bytes) {
            let sample = sample.unwrap();
            if loaded_elfs
                .as_ref()
                .is_none_or(|previous| !Arc::ptr_eq(previous, &sample.loaded_elfs))
            {
                symbolizer = Symbolizer::new(&sample.loaded_elfs);
                loaded_elfs = Some(Arc::clone(&sample.loaded_elfs));
            }
            let output = Sample {
                count: Count {
                    acc_bytes: sample.counts.acc_bytes,
                    alive_bytes: sample.counts.alive_bytes,
                    peak_alive_bytes: sample.counts.peak_alive_bytes,
                    realloc_growth_count: sample.counts.realloc_growth_count,
                    realloc_shrink_count: sample.counts.realloc_shrink_count,
                },
                call_chain: sample
                    .call_chain
                    .into_iter()
                    .map(|address| symbolizer.symbolize(address))
                    .collect(),
            };
            sequence.serialize_element(&output).unwrap();
        }
        sequence.end().unwrap();
    }
    writer.write_all(b"\n").unwrap();
    Ok(())
}

fn byte_stream(path: &Path) -> impl Iterator<Item = u8> {
    BufReader::new(File::open(path).unwrap())
        .bytes()
        .map(|byte| byte.unwrap())
}

impl Symbolizer {
    fn new(images: &[(u64, PathBuf)]) -> Self {
        let mut modules = Vec::new();

        if let Some((base, path)) = images.first()
            && let Some(mut module) = Module::load(path, true)
        {
            module.base = Some(*base);
            modules.push(module);
        }

        for (base, path) in images.iter().skip(1) {
            if let Some(mut module) = Module::load(path, false) {
                module.base = Some(*base);
                modules.push(module);
            }
        }

        Self { modules }
    }

    fn symbolize(&self, address: u64) -> String {
        if address == 0 {
            return "<null>".to_string();
        }
        for module in &self.modules {
            if let Some(symbol) = module.resolve(address) {
                return symbol;
            }
        }
        format!("0x{address:x}")
    }
}

impl Module {
    fn load(path: &Path, with_context: bool) -> Option<Self> {
        let bytes = fs::read(path).ok()?;
        let object = object::File::parse(&*bytes).ok()?;

        let segments = object
            .segments()
            .map(|segment| (segment.address(), segment.size()))
            .collect::<Vec<_>>();
        let mut debug_path = path.as_os_str().to_os_string();
        debug_path.push(".dSYM");
        let debug_bytes = if with_context {
            path.file_name().and_then(|name| {
                fs::read(
                    PathBuf::from(debug_path)
                        .join("Contents/Resources/DWARF")
                        .join(name),
                )
                .ok()
            })
        } else {
            None
        };
        let debug_object = debug_bytes
            .as_deref()
            .and_then(|bytes| object::File::parse(bytes).ok())
            .filter(|debug| {
                object
                    .mach_uuid()
                    .ok()
                    .flatten()
                    .is_some_and(|uuid| debug.mach_uuid().ok().flatten() == Some(uuid))
            });
        let context = if with_context {
            let object = debug_object.as_ref().unwrap_or(&object);
            let endian = if object.is_little_endian() {
                gimli::RunTimeEndian::Little
            } else {
                gimli::RunTimeEndian::Big
            };
            gimli::Dwarf::load(|id| -> Result<Reader, gimli::Error> {
                let data = object
                    .section_by_name(id.name())
                    .and_then(|section| section.data().ok())
                    .map(Arc::<[u8]>::from)
                    .unwrap_or_else(|| Arc::<[u8]>::from([]));
                Ok(gimli::EndianArcSlice::new(data, endian))
            })
            .ok()
            .and_then(|dwarf| Context::from_dwarf(dwarf).ok())
        } else {
            None
        };

        let mut symbols = object
            .symbols()
            .filter_map(|symbol| {
                let name = symbol.name().ok()?;
                let address = symbol.address();
                if address == 0 {
                    return None;
                }
                let name = addr2line::demangle_auto(Cow::Borrowed(name), None).into_owned();
                Some((address, symbol.size(), name))
            })
            .collect::<Vec<_>>();
        symbols.sort_by_key(|symbol| symbol.0);

        Some(Self {
            context,
            symbols,
            segments,
            base: None,
        })
    }

    fn resolve(&self, address: u64) -> Option<String> {
        let base = self.base?;
        if address < base || !address_in_segments(&self.segments, base, address) {
            return None;
        }

        let rva = address - base;

        if let Some(context) = self.context.as_ref()
            && let Ok(mut frames) = context.find_frames(rva).skip_all_loads()
        {
            while let Ok(Some(frame)) = frames.next() {
                let name = frame
                    .function
                    .as_ref()
                    .and_then(|function| function.demangle().ok())
                    .map(|name| name.into_owned());
                let location = frame.location.as_ref().and_then(|location| {
                    let file = location.file?;
                    Some(match location.line {
                        Some(line) => format!("{file}:{line}"),
                        None => file.to_string(),
                    })
                });

                if let Some(symbol) = merge_symbol(name, location) {
                    return Some(symbol);
                }
            }
        }

        lookup_symbol(&self.symbols, rva).map(|(_, _, name)| name.clone())
    }
}

fn merge_symbol(name: Option<String>, location: Option<String>) -> Option<String> {
    match (name, location) {
        (Some(name), Some(location)) => Some(format!("{name} ({location})")),
        (Some(name), None) => Some(name),
        (None, Some(location)) => Some(location),
        (None, None) => None,
    }
}

fn lookup_symbol(symbols: &[(u64, u64, String)], address: u64) -> Option<&(u64, u64, String)> {
    let index = match symbols.binary_search_by_key(&address, |symbol| symbol.0) {
        Ok(index) => index,
        Err(0) => return None,
        Err(index) => index - 1,
    };
    let symbol = &symbols[index];
    let end = if symbol.1 == 0 {
        symbols[index + 1..]
            .iter()
            .find(|next| next.0 > symbol.0)?
            .0
    } else {
        symbol.0.saturating_add(symbol.1)
    };
    if address >= end {
        return None;
    }
    Some(symbol)
}

fn address_in_segments(segments: &[(u64, u64)], base: u64, address: u64) -> bool {
    segments.iter().any(|&(vaddr, size)| {
        let start = base.saturating_add(vaddr);
        let end = start.saturating_add(size);
        address >= start && address < end
    })
}
